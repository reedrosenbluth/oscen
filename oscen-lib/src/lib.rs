extern crate self as oscen;

pub mod delay;
pub mod envelope;
pub mod event_passthrough;
pub mod filters;
pub mod gain;
pub mod graph;
pub mod midi;
pub mod oscilloscope;
pub mod oscillators;
pub mod ring_buffer;
pub mod value;
pub mod voice_allocator;

pub use delay::Delay;
pub use envelope::AdsrEnvelope;
pub use event_passthrough::EventPassthrough;
pub use filters::tpt::TptFilter;
pub use graph::*;
pub use midi::{
    queue_raw_midi, raw_midi_event, MidiParser, MidiVoiceHandler, NoteOffEvent, NoteOnEvent,
    RawMidiMessage,
};
pub use oscen_macros::{graph, Node};
pub use oscilloscope::{
    Oscilloscope, OscilloscopeEndpoints, OscilloscopeHandle, OscilloscopeSnapshot,
    DEFAULT_SCOPE_CAPACITY,
};
pub use oscillators::{
    Oscillator, OscillatorEndpoints, PolyBlepOscillator, PolyBlepOscillatorEndpoints,
};
pub use gain::{Gain, GainEndpoints};
pub use value::Value;
pub use voice_allocator::{
    VoiceAllocator, VoiceAllocator2, VoiceAllocator2Endpoints, VoiceAllocator4,
    VoiceAllocator4Endpoints, VoiceAllocatorEndpoints,
};
