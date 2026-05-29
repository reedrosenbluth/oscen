//! Integration test for `impl Sum<f32> for StreamInput`.
//!
//! When an array of nodes whose stream output is a raw `f32` fans into a single
//! node whose stream input is the typed `StreamInput` wrapper, the `graph!`
//! macro lowers the connection to:
//!
//! ```ignore
//! self.sink.input = self.voices.iter().map(|n| n.out).sum();
//! ```
//!
//! Here the iterator yields `f32` and the destination is `StreamInput<f32>`, so
//! the summation selects `impl Sum<f32> for StreamInput`. Without that impl this
//! generated code fails to compile (`StreamInput: Sum<f32>` unsatisfied).
//!
//! Note: this raw-f32-output → typed-`StreamInput` shape is what makes the new
//! impl necessary. Fan-ins into a bare `f32` input (e.g. the electric-piano
//! tremolo) use the stdlib `Sum<f32> for f32` instead, and fan-ins into an `f32`
//! graph output from `StreamOutput` voices use `Sum<StreamOutput> for f32`.
#![feature(inherent_associated_types)]

use oscen::graph::StreamInput;
use oscen::{graph, Node, SignalProcessor};

/// Voice element: emits its configured `value` on a raw-`f32` stream output.
/// The raw-`f32` output (not the `StreamOutput` wrapper) is what makes the
/// per-element `map` yield `f32`.
#[derive(Debug, Node)]
pub struct ConstVoice {
    #[output(stream)]
    pub out: f32,
    pub value: f32,
}

impl ConstVoice {
    pub fn new(value: f32) -> Self {
        Self { out: 0.0, value }
    }
}

impl Default for ConstVoice {
    fn default() -> Self {
        Self::new(0.0)
    }
}

impl SignalProcessor for ConstVoice {
    #[inline(always)]
    fn process(&mut self) {
        self.out = self.value;
    }
}

/// Mixer: its stream input is the typed `StreamInput` wrapper, so the fan-in
/// assignment targets a `StreamInput<f32>` and the summed result must itself be
/// a `StreamInput`. Passes the summed input straight through to its output.
#[derive(Debug, Node)]
pub struct WrappedMixer {
    pub input: StreamInput,
    #[output(stream)]
    pub out: f32,
}

impl WrappedMixer {
    pub fn new() -> Self {
        Self {
            input: StreamInput::default(),
            out: 0.0,
        }
    }
}

impl Default for WrappedMixer {
    fn default() -> Self {
        Self::new()
    }
}

impl SignalProcessor for WrappedMixer {
    #[inline(always)]
    fn process(&mut self) {
        self.out = *self.input;
    }
}

graph! {
    name: RawF32ArrayFanInToStreamInput;
    output stream out;

    nodes {
        voices = [ConstVoice::new(0.0); 4];
        mixer = WrappedMixer::new();
    }

    connections {
        voices.out -> mixer.input;
        mixer.out  -> out;
    }
}

#[test]
fn raw_f32_array_fans_into_stream_input_by_summing() {
    let mut g = RawF32ArrayFanInToStreamInput::new();
    g.init(48_000.0);

    // Distinct values so the assertion distinguishes a true sum (10.0) from a
    // broadcast or last-writer-wins (which would land on 4.0).
    g.voices[0].value = 1.0;
    g.voices[1].value = 2.0;
    g.voices[2].value = 3.0;
    g.voices[3].value = 4.0;

    g.process();

    // The fan-in summed the four raw-f32 outputs into the typed StreamInput.
    assert!(
        (*g.mixer.input - 10.0).abs() < 1e-6,
        "mixer.input (StreamInput) = {}, expected 10.0",
        *g.mixer.input
    );
    // And the summed value propagated through the mixer to the graph output.
    assert!(
        (g.out - 10.0).abs() < 1e-6,
        "graph out = {}, expected 10.0",
        g.out
    );
}
