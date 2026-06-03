use core::marker::PhantomData;

use super::{StreamDownsampler, StreamUpsampler};
use crate::frame::AudioFrame;

/// Zero-order-hold upsampler: emits the same source frame N times.
#[derive(Debug, Clone, Default)]
pub struct LatchUp<const N: usize, F: AudioFrame = f32>(PhantomData<F>);

impl<const N: usize, F: AudioFrame> LatchUp<N, F> {
    pub const fn new() -> Self {
        Self(PhantomData)
    }
}

impl<const N: usize, F: AudioFrame> StreamUpsampler<F> for LatchUp<N, F> {
    #[inline]
    fn upsample(&mut self, x: F, out: &mut [F]) {
        debug_assert_eq!(out.len(), N);
        for slot in out.iter_mut() {
            *slot = x;
        }
    }
    #[inline]
    fn latency_samples(&self) -> usize {
        0
    }
    #[inline]
    fn reset(&mut self) {}
}

/// Zero-order-hold downsampler: takes the first of every N source frames.
#[derive(Debug, Clone, Default)]
pub struct LatchDown<const N: usize, F: AudioFrame = f32>(PhantomData<F>);

impl<const N: usize, F: AudioFrame> LatchDown<N, F> {
    pub const fn new() -> Self {
        Self(PhantomData)
    }
}

impl<const N: usize, F: AudioFrame> StreamDownsampler<F> for LatchDown<N, F> {
    #[inline]
    fn downsample(&mut self, xs: &[F]) -> F {
        debug_assert_eq!(xs.len(), N);
        xs[0]
    }
    #[inline]
    fn latency_samples(&self) -> usize {
        0
    }
    #[inline]
    fn reset(&mut self) {}
}
