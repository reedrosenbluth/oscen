//! Prelude module for oscen - import commonly used items with `use oscen::prelude::*;`

// Core graph types and traits
pub use crate::graph::SignalProcessor;

// Macro for building graphs
pub use crate::graph;

// Common endpoint types
pub use crate::{EventInput, EventOutput};

// Common nodes
pub use crate::{
    AdsrEnvelope, AudioInput, Delay, Gain, IirLowpass, Oscillator, PolyBlepOscillator, TptFilter,
};

// Sample loading, sharing, and playback
pub use crate::sample::{self, SampleBank, SampleBuffer, SamplePlayer, SampleSlot};

// MIDI and voice management
pub use crate::{MidiParser, MidiVoiceHandler, VoiceAllocator};

// Value system
pub use crate::Value;

// Multi-channel frame value type
pub use crate::{AudioFrame, Frame};
