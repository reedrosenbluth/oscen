//! Integration test for array fan-in into a single stream input.
//!
//! When an array of nodes whose stream output is an `f32` fans into a single
//! node's `f32` stream input, the `graph!` macro lowers the connection to:
//!
//! ```ignore
//! self.sink.input = self.voices.iter().map(|n| n.out).sum();
//! ```
//!
//! The iterator yields `f32` and the destination is `f32`, so summation uses
//! the std `Sum<f32> for f32` impl. This guards the fan-in lowering shape.
#![feature(inherent_associated_types)]

use oscen::{graph, Node, SignalProcessor};

/// Voice element: emits its configured `value` on an `f32` stream output.
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

/// Mixer: the fan-in assignment targets its `f32` stream input. Passes the
/// summed input straight through to its output.
#[derive(Debug, Node)]
pub struct WrappedMixer {
    #[input(stream)]
    pub input: f32,
    #[output(stream)]
    pub out: f32,
}

impl WrappedMixer {
    pub fn new() -> Self {
        Self {
            input: Default::default(),
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
        self.out = self.input;
    }
}

graph! {
    name: ArrayFanInToStreamInput;
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
    let mut g = ArrayFanInToStreamInput::new();
    g.init(48_000.0);

    // Distinct values so the assertion distinguishes a true sum (10.0) from a
    // broadcast or last-writer-wins (which would land on 4.0).
    g.voices[0].value = 1.0;
    g.voices[1].value = 2.0;
    g.voices[2].value = 3.0;
    g.voices[3].value = 4.0;

    g.process();

    // The fan-in summed the four f32 outputs into the mixer's input.
    assert!(
        (g.mixer.input - 10.0).abs() < 1e-6,
        "mixer.input = {}, expected 10.0",
        g.mixer.input
    );
    // And the summed value propagated through the mixer to the graph output.
    assert!(
        (g.out - 10.0).abs() < 1e-6,
        "graph out = {}, expected 10.0",
        g.out
    );
}
