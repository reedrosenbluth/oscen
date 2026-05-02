use super::coeffs::{HALFBAND_23_CENTER, HALFBAND_23_GROUP_DELAY, HALFBAND_23_HALF};
use super::{StreamDownsampler, StreamUpsampler};

const HB_LEN: usize = 23;
const HB_CENTER_IDX: usize = 11; // center tap index of the 23-tap filter
const HB_PAIR_SUM: usize = HB_LEN - 1; // = 22; symmetric pair indices sum to this

/// One stage of 2× upsample using a 23-tap symmetric halfband FIR.
///
/// Polyphase decomposition. The 23-tap halfband splits into two L=2 polyphase
/// sub-filters:
///   * E_even (taps at filter indices 0, 2, 4, ..., 22): all 12 even-index taps
///     are non-zero; produces y[2n].
///   * E_odd  (taps at filter indices 1, 3, ..., 21): only the center tap
///     h[11] = 0.5 is non-zero; produces y[2n+1] = 0.5 * x[n - 5].
///
/// Because `firwin` normalises sum h = 1, each branch sums to 0.5 → DC gain 0.5
/// per output sample. We compensate by scaling each output by 2 so the
/// upsampler has unity DC gain, matching the downsampler.
/// Maximum low-rate delay needed by the up stage's polyphase filter. The
/// 23-tap halfband has its 12 even-indexed taps spanning low-rate delays 0..11
/// after polyphase decomposition, so we need 12 slots.
const HB_UP_HISTORY: usize = (HB_LEN + 1) / 2; // 12
/// Largest low-rate delay used by the FIR branch / center-tap branch (= 11).
const HB_UP_MAX_DELAY: usize = HB_UP_HISTORY - 1;

#[derive(Debug, Clone)]
struct Halfband2xUpStage {
    /// Stores the most recent low-rate input samples. `head` points to the
    /// slot of the *most recent* sample (i.e. the sample we just wrote).
    history: [f32; HB_UP_HISTORY],
    head: usize,
}

impl Halfband2xUpStage {
    fn new() -> Self {
        Self {
            history: [0.0; HB_UP_HISTORY],
            head: 0,
        }
    }

    /// Push one source sample, write 2 destination samples to `out`.
    #[inline]
    fn step(&mut self, x: f32, out: &mut [f32; 2]) {
        let cap = self.history.len();
        // Advance head, then write at the new head position so `at(0)` = newest.
        self.head = (self.head + 1) % cap;
        self.history[self.head] = x;

        // Helper: at(d) returns x[n - d] for d=0..(cap-1), with d=0 being the
        // sample we just stored (newest).
        let at = |d: usize| -> f32 { self.history[(self.head + cap - d) % cap] };

        // Polyphase E_odd branch — only the center tap h[11] is non-zero.
        // y[2n+1] = h[11] * x[n - 5] = 0.5 * x[n - 5].
        // Center delay in low-rate samples = (HB_CENTER_IDX - 1) / 2 = 5.
        // We multiply by 2 to compensate the 0.5 zero-stuffing loss.
        out[1] = 2.0 * HALFBAND_23_CENTER * at((HB_CENTER_IDX - 1) / 2);

        // Polyphase E_even branch — 12 even-index taps mapped to low-rate
        // delays via m/2. y[2n] = sum_{k=0..5} h[2k] * (x[n-k] + x[n-(HB_UP_MAX_DELAY-k)]).
        // Filter symmetry h[2k] = h[22-2k] = HALFBAND_23_HALF[k].
        let mut acc = 0.0_f32;
        for (k, &tap) in HALFBAND_23_HALF.iter().enumerate() {
            let left = at(k);
            let right = at(HB_UP_MAX_DELAY - k);
            acc += tap * (left + right);
        }
        out[0] = 2.0 * acc;
    }

    fn reset(&mut self) {
        self.history.fill(0.0);
        self.head = 0;
    }
}

/// One stage of 2× downsample using the same halfband.
///
/// Convolves the 23-tap halfband with the high-rate input stream and outputs
/// the FIR result evaluated at the EVEN high-rate index of each pair (i.e.
/// `y[m] = (h * x)[2m]`). Sampling at the even index means the centre tap
/// aligns with `xs[0]` of pair `m+5` (5 pairs after the impulse), so that the
/// up→down cascade has a single impulse peak at low-rate index 11 instead of
/// splitting between m=10 and m=11. Buffer size is `HB_LEN + 1` so we can
/// access delays 0..22 from the older sample of the pair.
const HB_DOWN_BUF: usize = HB_LEN + 1; // 24

#[derive(Debug, Clone)]
struct Halfband2xDownStage {
    /// Stores the most recent high-rate input samples. `head` points to the
    /// slot of the *most recent* sample (i.e. `xs[1]` after push).
    history: [f32; HB_DOWN_BUF],
    head: usize,
}

impl Halfband2xDownStage {
    fn new() -> Self {
        Self {
            history: [0.0; HB_DOWN_BUF],
            head: 0,
        }
    }

    /// Push two source samples, return one destination sample.
    #[inline]
    fn step(&mut self, xs: &[f32; 2]) -> f32 {
        let cap = HB_DOWN_BUF;
        // Advance head and store the two new samples in arrival order.
        self.head = (self.head + 1) % cap;
        self.history[self.head] = xs[0];
        self.head = (self.head + 1) % cap;
        self.history[self.head] = xs[1];

        // We want y[m] = (h * x)[2m] = sum_k h[k] * x[2m - k].
        // The newest sample written is xs[1] = x[2m + 1]. The older sample of
        // this pair is xs[0] = x[2m]. So `at(d) = x[2m - d]` should read the
        // slot one back from `head`. Using `head + cap - 1 - d` mod cap.
        let at = |d: usize| -> f32 { self.history[(self.head + cap - 1 - d) % cap] };

        // Center tap at delay HB_CENTER_IDX = 11 in high-rate samples.
        let mut acc = HALFBAND_23_CENTER * at(HB_CENTER_IDX);

        // Symmetric off-center pairs: filter indices (2k, 22 - 2k) → high-rate
        // delays (2k, 22 - 2k) = (2k, HB_PAIR_SUM - 2k).
        for (k, &tap) in HALFBAND_23_HALF.iter().enumerate() {
            let left = at(2 * k);
            let right = at(HB_PAIR_SUM - 2 * k);
            acc += tap * (left + right);
        }
        acc
    }

    fn reset(&mut self) {
        self.history.fill(0.0);
        self.head = 0;
    }
}

/// Sinc-FIR upsampler for N ∈ {1, 2, 4, 8}. Cascades 2× halfband stages.
#[derive(Debug, Clone)]
pub struct SincUpFir<const N: usize> {
    stages: Vec<Halfband2xUpStage>,
}

impl<const N: usize> SincUpFir<N> {
    pub fn new() -> Self {
        const_assert_pow2_le_8::<N>();
        let n_stages = (N as u32).trailing_zeros() as usize; // 0,1,2,3 for N=1,2,4,8
        Self {
            stages: (0..n_stages).map(|_| Halfband2xUpStage::new()).collect(),
        }
    }
}

impl<const N: usize> Default for SincUpFir<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> StreamUpsampler for SincUpFir<N> {
    fn upsample(&mut self, x: f32, out: &mut [f32]) {
        debug_assert_eq!(out.len(), N);
        // Cascaded stages: each doubles the rate. We do this in-place via a
        // small temporary buffer per stage.
        let mut buf: [f32; 8] = [0.0; 8]; // max N = 8
        let mut next: [f32; 8] = [0.0; 8];
        let mut len = 1;
        buf[0] = x;
        for stage in self.stages.iter_mut() {
            for i in 0..len {
                let mut pair = [0.0_f32; 2];
                stage.step(buf[i], &mut pair);
                next[2 * i] = pair[0];
                next[2 * i + 1] = pair[1];
            }
            len *= 2;
            buf[..len].copy_from_slice(&next[..len]);
        }
        out.copy_from_slice(&buf[..N]);
    }
    fn latency_samples(&self) -> usize {
        // Each stage adds GROUP_DELAY at its output rate. For cascaded stages,
        // total at final dest (high) rate = GROUP_DELAY * (2^n - 1) for n stages.
        let n = self.stages.len();
        if n == 0 {
            0
        } else {
            HALFBAND_23_GROUP_DELAY * ((1 << n) - 1)
        }
    }
    fn reset(&mut self) {
        for s in &mut self.stages {
            s.reset();
        }
    }
}

/// Sinc-FIR downsampler for N ∈ {1, 2, 4, 8}.
#[derive(Debug, Clone)]
pub struct SincDownFir<const N: usize> {
    stages: Vec<Halfband2xDownStage>,
}

impl<const N: usize> SincDownFir<N> {
    pub fn new() -> Self {
        const_assert_pow2_le_8::<N>();
        let n_stages = (N as u32).trailing_zeros() as usize;
        Self {
            stages: (0..n_stages).map(|_| Halfband2xDownStage::new()).collect(),
        }
    }
}

impl<const N: usize> Default for SincDownFir<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> StreamDownsampler for SincDownFir<N> {
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
                next[i] = stage.step(&pair);
            }
            len = half;
            buf[..len].copy_from_slice(&next[..len]);
        }
        buf[0]
    }
    fn latency_samples(&self) -> usize {
        // Reported at source (high) rate. Each stage's group delay is GROUP_DELAY
        // at the stage's input rate. Total source-rate latency for n_stages =
        // log2(N) is GROUP_DELAY * (N - 1).
        let n = self.stages.len();
        if n == 0 {
            0
        } else {
            HALFBAND_23_GROUP_DELAY * ((1 << n) - 1)
        }
    }
    fn reset(&mut self) {
        for s in &mut self.stages {
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
