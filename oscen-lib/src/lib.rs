extern crate self as oscen;

pub mod delay;
pub mod envelope;
pub mod event_passthrough;
pub mod filters;
pub mod gain;
pub mod graph;
pub mod midi;
pub mod oscillators;
pub mod oscilloscope;
pub mod ring_buffer;
pub mod value;
pub mod voice_allocator;

// Re-export everything from each module to make it easy for consumers
pub use delay::*;
pub use envelope::*;
pub use event_passthrough::*;
pub use filters::iir_lowpass::*;
pub use filters::tpt::*;
pub use gain::*;
pub use graph::*;
pub use midi::*;
pub use oscen_macros::{graph, Node};
pub use oscillators::*;
pub use oscilloscope::*;
pub use value::*;
pub use voice_allocator::*;
