use super::{StreamDownsampler, StreamUpsampler};

/// Linear-interpolation upsampler.
///
/// Produces N output samples linearly interpolated between the previous and
/// current source samples. Adds 1 destination-sample of group delay.
#[derive(Debug, Clone, Default)]
pub struct LinearUp<const N: usize> {
    prev: f32,
}

impl<const N: usize> LinearUp<N> {
    pub const fn new() -> Self {
        Self { prev: 0.0 }
    }
}

impl<const N: usize> StreamUpsampler for LinearUp<N> {
    #[inline]
    fn upsample(&mut self, x: f32, out: &mut [f32]) {
        debug_assert_eq!(out.len(), N);
        let n_inv = 1.0 / N as f32;
        let delta = x - self.prev;
        for i in 0..N {
            out[i] = self.prev + delta * (i as f32) * n_inv;
        }
        self.prev = x;
    }
    #[inline]
    fn latency_samples(&self) -> usize {
        1
    }
    #[inline]
    fn reset(&mut self) {
        self.prev = 0.0;
    }
}

/// Linear-interpolation downsampler.
///
/// Returns the arithmetic mean of the N source samples (a moving-average box
/// filter equivalent to a 1st-order linear interpolator at the dest grid).
#[derive(Debug, Clone, Default)]
pub struct LinearDown<const N: usize>;

impl<const N: usize> LinearDown<N> {
    pub const fn new() -> Self {
        Self
    }
}

impl<const N: usize> StreamDownsampler for LinearDown<N> {
    #[inline]
    fn downsample(&mut self, xs: &[f32]) -> f32 {
        debug_assert_eq!(xs.len(), N);
        let mut acc = 0.0;
        for &x in xs {
            acc += x;
        }
        acc / N as f32
    }
    #[inline]
    fn latency_samples(&self) -> usize {
        0
    }
    #[inline]
    fn reset(&mut self) {}
}
