//! Sample-rate conversion kernels used by the multi-rate `graph!` macro.
//!
//! These kernels are not graph nodes — they're plain structs that hold per-edge
//! resampler state. The macro generates fields of these types in the graph struct
//! and calls them from the inner loop of `process_block`.

pub mod coeffs;
pub mod halfband_iir;
pub mod latch;
pub mod linear;
pub mod sinc_fir;

// Re-exported in Tasks 1.2-1.6
pub use halfband_iir::{IirHalfbandDown, IirHalfbandUp};
pub use latch::{LatchDown, LatchUp};
pub use linear::{LinearDown, LinearUp};
pub use sinc_fir::{SincDownFir, SincUpFir};

use crate::frame::AudioFrame;

/// Upsampler: one source frame in, N destination frames out.
pub trait StreamUpsampler<F: AudioFrame = f32>: Send + std::fmt::Debug {
    /// Push one source frame; the kernel writes exactly `N` destination frames to `out`.
    fn upsample(&mut self, x: F, out: &mut [F]);

    /// Group delay measured at the destination (high) rate, in samples.
    fn latency_samples(&self) -> usize;

    /// Clear all internal state.
    fn reset(&mut self);
}

/// Downsampler: N source frames in, one destination frame out.
pub trait StreamDownsampler<F: AudioFrame = f32>: Send + std::fmt::Debug {
    /// Push exactly `N` source frames; returns one destination frame.
    fn downsample(&mut self, xs: &[F]) -> F;

    /// Group delay measured at the source (high) rate, in samples.
    fn latency_samples(&self) -> usize;

    /// Clear all internal state.
    fn reset(&mut self);
}
