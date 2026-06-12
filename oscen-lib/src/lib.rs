#![feature(inherent_associated_types)]

extern crate self as oscen;

#[cfg(feature = "convolution")]
pub mod convolution;
pub mod delay;
pub mod dispatch;
pub mod envelope;
pub mod event_passthrough;
pub mod filters;
pub mod frame;
pub mod gain;
pub mod graph;
pub mod midi;
pub mod oscillators;
pub mod oscilloscope;
pub mod prelude;
pub mod resample;
pub mod ring_buffer;
#[cfg(feature = "fft")]
pub mod spectral;
pub mod value;
pub mod voice_allocator;

#[cfg(test)]
mod multi_channel_test;

// Re-export everything from each module to make it easy for consumers
#[cfg(feature = "convolution")]
pub use convolution::*;
pub use delay::*;
pub use dispatch::*;
pub use envelope::*;
pub use event_passthrough::*;
pub use filters::iir_lowpass::*;
pub use filters::tpt::*;
pub use frame::*;
pub use gain::*;
pub use graph::*;
pub use midi::*;
pub use oscen_macros::{graph, oversample_variants, Node};
pub use oscillators::*;
pub use oscilloscope::*;
pub use value::*;
pub use voice_allocator::*;
