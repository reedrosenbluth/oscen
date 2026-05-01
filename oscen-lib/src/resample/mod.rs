//! Sample-rate conversion kernels used by the multi-rate `graph!` macro.
//!
//! These kernels are not graph nodes — they're plain structs that hold per-edge
//! resampler state. The macro generates fields of these types in the graph struct
//! and calls them from the inner loop of `process_block`.

pub mod latch;
pub mod linear;
pub mod sinc_fir;
pub mod halfband_iir;
pub mod coeffs;

// Re-exported in Tasks 1.2-1.6
// pub use latch::{LatchDown, LatchUp};
// pub use linear::{LinearDown, LinearUp};
// pub use sinc_fir::{SincDownFir, SincUpFir};
// pub use halfband_iir::{IirHalfbandDown, IirHalfbandUp};

/// Upsampler: one source sample in, N destination samples out.
pub trait StreamUpsampler: Send + std::fmt::Debug {
    /// Push one source sample; the kernel writes exactly `N` destination samples to `out`.
    fn upsample(&mut self, x: f32, out: &mut [f32]);

    /// Group delay measured at the destination (high) rate, in samples.
    fn latency_samples(&self) -> usize;

    /// Clear all internal state.
    fn reset(&mut self);
}

/// Downsampler: N source samples in, one destination sample out.
pub trait StreamDownsampler: Send + std::fmt::Debug {
    /// Push exactly `N` source samples; returns one destination sample.
    fn downsample(&mut self, xs: &[f32]) -> f32;

    /// Group delay measured at the source (high) rate, in samples.
    fn latency_samples(&self) -> usize;

    /// Clear all internal state.
    fn reset(&mut self);
}
