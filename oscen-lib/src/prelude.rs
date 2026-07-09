//! Prelude module for oscen - import commonly used items with `use oscen::prelude::*;`

// Core graph types and traits
pub use crate::graph::SignalProcessor;

// Offline (non-realtime) rendering
pub use crate::graph::BlockRender;

// Macro for building graphs
pub use crate::graph;

// Common endpoint types
pub use crate::{EventInput, EventOutput};

// Parameter reflection (generated `PARAMS` tables use this descriptor type)
pub use crate::graph::ParamDescriptor;

// Opt-in marker for typed value endpoint payloads (enums, bools, small
// Copy structs): `impl ValuePayload for MyType {}`.
pub use crate::graph::ValuePayload;

// Common nodes
#[cfg(feature = "convolution")]
pub use crate::convolution::Convolver;
pub use crate::{
    AdsrEnvelope, AudioInput, Delay, Gain, IirLowpass, Oscillator, PolyBlepOscillator,
    SamplePlayer, TptFilter,
};

// MIDI and voice management
pub use crate::{MidiParser, MidiVoiceHandler, VoiceAllocator};

// Endpoint manifest aliases for the node types re-exported above. The graph!
// macro resolves a bare `Type::new(...)` constructor path to a bare
// `__oscen_endpoints_<Type>!` invocation, so wildcard hoists need the
// manifest alias in scope wherever the type name is.
#[cfg(feature = "convolution")]
#[doc(hidden)]
pub use crate::__oscen_endpoints_Convolver;
#[doc(hidden)]
pub use crate::{
    __oscen_endpoints_AdsrEnvelope, __oscen_endpoints_AudioInput, __oscen_endpoints_Delay,
    __oscen_endpoints_Gain, __oscen_endpoints_IirLowpass, __oscen_endpoints_MidiParser,
    __oscen_endpoints_MidiVoiceHandler, __oscen_endpoints_Oscillator,
    __oscen_endpoints_PolyBlepOscillator, __oscen_endpoints_SamplePlayer,
    __oscen_endpoints_TptFilter, __oscen_endpoints_Value, __oscen_endpoints_VoiceAllocator,
};

// Value system
pub use crate::Value;

// Multi-channel frame value type
pub use crate::{AudioFrame, Frame};

// Audio assets (immutable sample buffers)
pub use crate::{AssetError, AudioAsset};
