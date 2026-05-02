//! IIR halfband resamplers built from a two-branch all-pass cascade.
//!
//! Polyphase form: `H(z) = 0.5 * (A(z^2) + z^-1 * B(z^2))` where A and B are
//! cascades of 1st-order all-pass sections of the form `(a + z^-1)/(1 + a*z^-1)`
//! (operating at the low rate after polyphase decomposition).
//!
//! Compared with the linear-phase 23-tap FIR in [`super::sinc_fir`] this trades
//! a few dB of stopband for an order of magnitude lower latency, which is
//! crucial inside oversampled feedback paths.

use arrayvec::ArrayVec;

use super::coeffs::{BRANCH_A_BETAS, BRANCH_B_BETAS, IIR_HALFBAND_GROUP_DELAY};
use super::{StreamDownsampler, StreamUpsampler};

/// Maximum number of cascaded 2× halfband stages (8× = 3 stages).
const MAX_STAGES: usize = 3;

/// Single 1st-order all-pass section: `y[n] = a * (x[n] - y[n-1]) + x[n-1]`.
///
/// This is the lattice form of `H(z) = (a + z^-1)/(1 + a*z^-1)`. We store the
/// previous input `x_prev` and previous output `y_prev`.
#[derive(Debug, Clone, Copy)]
struct Allpass1 {
    a: f32,
    x_prev: f32,
    y_prev: f32,
}

impl Allpass1 {
    const fn new(a: f32) -> Self {
        Self {
            a,
            x_prev: 0.0,
            y_prev: 0.0,
        }
    }

    #[inline]
    fn step(&mut self, x: f32) -> f32 {
        let y = self.a * (x - self.y_prev) + self.x_prev;
        self.x_prev = x;
        self.y_prev = y;
        y
    }

    fn reset(&mut self) {
        self.x_prev = 0.0;
        self.y_prev = 0.0;
    }
}

/// One 2× halfband stage: a polyphase pair of all-pass cascades.
///
/// Each branch is a cascade of K 1st-order all-pass sections (here K = 2)
/// running at the LOW rate. The high-rate transfer function is
/// `H(z) = 0.5 * (A(z^2) + z^-1 * B(z^2))`, so branch B has a one-high-rate-
/// sample delay relative to branch A. After polyphase decimation that delay
/// becomes a one-low-rate-step delay on branch B's input (downsampler) or a
/// half-sample interleave on its output (upsampler).
#[derive(Debug, Clone)]
struct IirHalfband2x {
    branch_a: [Allpass1; 2],
    branch_b: [Allpass1; 2],
    /// One-step delay buffer for branch B's input on the decimation path:
    /// the ODD high-rate sample we received last call, fed into branch B
    /// THIS call (so y[m] uses x_odd[m-1] as required by the polyphase form).
    prev_odd_in: f32,
}

impl IirHalfband2x {
    fn new() -> Self {
        Self {
            branch_a: [
                Allpass1::new(BRANCH_A_BETAS[0]),
                Allpass1::new(BRANCH_A_BETAS[1]),
            ],
            branch_b: [
                Allpass1::new(BRANCH_B_BETAS[0]),
                Allpass1::new(BRANCH_B_BETAS[1]),
            ],
            prev_odd_in: 0.0,
        }
    }

    /// 2× upsample: one low-rate sample in, two high-rate samples out.
    ///
    /// `y[2n]   = A(x[n])` (even-phase branch, no extra delay)
    /// `y[2n+1] = B(x[n])` (odd-phase branch; the z^-1 in the high-rate form
    /// is realised by interleaving B's output between consecutive A outputs).
    #[inline]
    fn step_up(&mut self, x: f32, out: &mut [f32; 2]) {
        let mut a = x;
        for s in self.branch_a.iter_mut() {
            a = s.step(a);
        }
        let mut b = x;
        for s in self.branch_b.iter_mut() {
            b = s.step(b);
        }
        out[0] = a;
        out[1] = b;
    }

    /// 2× downsample: two high-rate samples in, one low-rate sample out.
    ///
    /// Polyphase decimator: `y[m] = 0.5 * (A(x_even)[m] + B(x_odd)[m-1])`
    /// where `x_even[m] = xs[0]` and `x_odd[m] = xs[1]`. The required
    /// one-step delay on branch B's input is held in `prev_odd_in`.
    #[inline]
    fn step_down(&mut self, xs: &[f32; 2]) -> f32 {
        let mut a = xs[0];
        for s in self.branch_a.iter_mut() {
            a = s.step(a);
        }
        let mut b = self.prev_odd_in;
        for s in self.branch_b.iter_mut() {
            b = s.step(b);
        }
        self.prev_odd_in = xs[1];
        0.5 * (a + b)
    }

    fn reset(&mut self) {
        for s in self.branch_a.iter_mut() {
            s.reset();
        }
        for s in self.branch_b.iter_mut() {
            s.reset();
        }
        self.prev_odd_in = 0.0;
    }
}

/// IIR-halfband upsampler for `N ∈ {1, 2, 4, 8}`. Cascades 2× halfband stages.
#[derive(Debug, Clone)]
pub struct IirHalfbandUp<const N: usize> {
    stages: ArrayVec<IirHalfband2x, MAX_STAGES>,
}

impl<const N: usize> IirHalfbandUp<N> {
    pub fn new() -> Self {
        const_assert_pow2_le_8::<N>();
        let n_stages = (N as u32).trailing_zeros() as usize; // 0,1,2,3 for N=1,2,4,8
        let mut stages = ArrayVec::new();
        for _ in 0..n_stages {
            stages.push(IirHalfband2x::new());
        }
        Self { stages }
    }
}

impl<const N: usize> Default for IirHalfbandUp<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> StreamUpsampler for IirHalfbandUp<N> {
    fn upsample(&mut self, x: f32, out: &mut [f32]) {
        debug_assert_eq!(out.len(), N);
        let mut buf: [f32; 8] = [0.0; 8];
        let mut next: [f32; 8] = [0.0; 8];
        let mut len = 1;
        buf[0] = x;
        for stage in self.stages.iter_mut() {
            for i in 0..len {
                let mut pair = [0.0_f32; 2];
                stage.step_up(buf[i], &mut pair);
                next[2 * i] = pair[0];
                next[2 * i + 1] = pair[1];
            }
            len *= 2;
            buf[..len].copy_from_slice(&next[..len]);
        }
        out.copy_from_slice(&buf[..N]);
    }

    fn latency_samples(&self) -> usize {
        // Reported at the destination (high) rate, matching the trait docs.
        // Each 2× stage adds IIR_HALFBAND_GROUP_DELAY at its OUTPUT rate. For
        // n cascaded stages the total at the final high rate is
        // GROUP_DELAY * (2^n - 1).
        let n = self.stages.len();
        if n == 0 {
            0
        } else {
            IIR_HALFBAND_GROUP_DELAY * ((1 << n) - 1)
        }
    }

    fn reset(&mut self) {
        for s in self.stages.iter_mut() {
            s.reset();
        }
    }
}

/// IIR-halfband downsampler for `N ∈ {1, 2, 4, 8}`. Cascades 2× halfband stages.
#[derive(Debug, Clone)]
pub struct IirHalfbandDown<const N: usize> {
    stages: ArrayVec<IirHalfband2x, MAX_STAGES>,
}

impl<const N: usize> IirHalfbandDown<N> {
    pub fn new() -> Self {
        const_assert_pow2_le_8::<N>();
        let n_stages = (N as u32).trailing_zeros() as usize;
        let mut stages = ArrayVec::new();
        for _ in 0..n_stages {
            stages.push(IirHalfband2x::new());
        }
        Self { stages }
    }
}

impl<const N: usize> Default for IirHalfbandDown<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> StreamDownsampler for IirHalfbandDown<N> {
    fn downsample(&mut self, xs: &[f32]) -> f32 {
        debug_assert_eq!(xs.len(), N);
        let mut buf: [f32; 8] = [0.0; 8];
        buf[..N].copy_from_slice(xs);
        let mut len = N;
        for stage in self.stages.iter_mut() {
            let mut next: [f32; 8] = [0.0; 8];
            let half = len / 2;
            for i in 0..half {
                let pair = [buf[2 * i], buf[2 * i + 1]];
                next[i] = stage.step_down(&pair);
            }
            len = half;
            buf[..len].copy_from_slice(&next[..len]);
        }
        buf[0]
    }

    fn latency_samples(&self) -> usize {
        // Reported at the source (high) rate. Same expression as the upsampler
        // since both are measured at the high rate.
        let n = self.stages.len();
        if n == 0 {
            0
        } else {
            IIR_HALFBAND_GROUP_DELAY * ((1 << n) - 1)
        }
    }

    fn reset(&mut self) {
        for s in self.stages.iter_mut() {
            s.reset();
        }
    }
}

/// Compile-time assert that N ∈ {1, 2, 4, 8}. (1 produces zero stages, valid no-op.)
const fn const_assert_pow2_le_8<const N: usize>() {
    assert!(
        N == 1 || N == 2 || N == 4 || N == 8,
        "N must be 1, 2, 4, or 8"
    );
}
