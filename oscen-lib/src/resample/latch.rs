use super::{StreamDownsampler, StreamUpsampler};

/// Zero-order-hold upsampler: emits the same source sample N times.
#[derive(Debug, Clone, Default)]
pub struct LatchUp<const N: usize>;

impl<const N: usize> LatchUp<N> {
    pub const fn new() -> Self {
        Self
    }
}

impl<const N: usize> StreamUpsampler for LatchUp<N> {
    #[inline]
    fn upsample(&mut self, x: f32, out: &mut [f32]) {
        debug_assert_eq!(out.len(), N);
        for slot in out.iter_mut() {
            *slot = x;
        }
    }
    #[inline]
    fn latency_samples(&self) -> usize { 0 }
    #[inline]
    fn reset(&mut self) {}
}

/// Zero-order-hold downsampler: takes the first of every N source samples.
#[derive(Debug, Clone, Default)]
pub struct LatchDown<const N: usize>;

impl<const N: usize> LatchDown<N> {
    pub const fn new() -> Self {
        Self
    }
}

impl<const N: usize> StreamDownsampler for LatchDown<N> {
    #[inline]
    fn downsample(&mut self, xs: &[f32]) -> f32 {
        debug_assert_eq!(xs.len(), N);
        xs[0]
    }
    #[inline]
    fn latency_samples(&self) -> usize { 0 }
    #[inline]
    fn reset(&mut self) {}
}
