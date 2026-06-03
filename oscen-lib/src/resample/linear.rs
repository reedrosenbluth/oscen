use core::marker::PhantomData;

use super::{StreamDownsampler, StreamUpsampler};
use crate::frame::AudioFrame;

/// Linear-interpolation upsampler.
///
/// Produces N output samples linearly interpolated between the previous and
/// current source samples. Adds N destination-samples of group delay (the
/// impulse-response peak lands at dest-rate index N).
#[derive(Debug, Clone, Default)]
pub struct LinearUp<const N: usize, F: AudioFrame = f32> {
    prev: F,
}

impl<const N: usize, F: AudioFrame> LinearUp<N, F> {
    pub fn new() -> Self {
        Self { prev: F::default() }
    }
}

impl<const N: usize, F: AudioFrame> StreamUpsampler<F> for LinearUp<N, F> {
    #[inline]
    fn upsample(&mut self, x: F, out: &mut [F]) {
        debug_assert_eq!(out.len(), N);
        let n_inv = 1.0 / N as f32;
        let delta = x - self.prev;
        for i in 0..N {
            out[i] = self.prev + delta * ((i as f32) * n_inv);
        }
        self.prev = x;
    }
    #[inline]
    fn latency_samples(&self) -> usize {
        N
    }
    #[inline]
    fn reset(&mut self) {
        self.prev = F::default();
    }
}

/// Linear-interpolation downsampler.
///
/// Returns the arithmetic mean of the N source samples (a moving-average box
/// filter equivalent to a 1st-order linear interpolator at the dest grid).
/// Group delay is (N-1)/2 source samples (symmetric N-tap moving average).
/// Reported as `usize`, so even N truncates (e.g. N=2 → 0, true value 0.5).
#[derive(Debug, Clone, Default)]
pub struct LinearDown<const N: usize, F: AudioFrame = f32>(PhantomData<F>);

impl<const N: usize, F: AudioFrame> LinearDown<N, F> {
    pub const fn new() -> Self {
        Self(PhantomData)
    }
}

impl<const N: usize, F: AudioFrame> StreamDownsampler<F> for LinearDown<N, F> {
    #[inline]
    fn downsample(&mut self, xs: &[F]) -> F {
        debug_assert_eq!(xs.len(), N);
        let mut acc = F::default();
        for &x in xs {
            acc = acc + x;
        }
        acc * (1.0 / N as f32)
    }
    #[inline]
    fn latency_samples(&self) -> usize {
        (N - 1) / 2
    }
    #[inline]
    fn reset(&mut self) {}
}
