//! Cross-rate fan-out integration tests for rate-annotated array nodes.
//! Exercises the four shapes (Scalar / Broadcast / FanIn / Parallel) across
//! stream / value / event endpoints over a rate boundary.

use oscen::graph::{StreamOutput, ValueInput, ValueOutput};
use oscen::{graph, Node, SignalProcessor};

/// Trivial value-passthrough node: copies its value input into its value
/// output every tick. Combined with a `LatchUp` cross-rate kernel, this
/// node lets a test observe whether the latched value reached every dest
/// element after an outer tick.
#[derive(Debug, Node)]
pub struct ValueLatch {
    pub input: ValueInput<f32>,
    pub output: ValueOutput<f32>,
}

impl ValueLatch {
    pub fn new() -> Self {
        Self {
            input: ValueInput::default(),
            output: ValueOutput(0.0),
        }
    }
}

impl Default for ValueLatch {
    fn default() -> Self {
        Self::new()
    }
}

impl SignalProcessor for ValueLatch {
    #[inline(always)]
    fn process(&mut self) {
        *self.output = *self.input;
    }
}

graph! {
    name: BroadcastValueOversampled;
    input value src = 0.0;
    nodes {
        latches = [ValueLatch::new(); 4] * 2;
    }
    connections {
        src -> latches.input;
    }
}

#[test]
fn broadcast_value_outer_to_oversampled_array() {
    let mut g = BroadcastValueOversampled::new();
    g.init(48_000.0);
    g.src = 0.7;
    for _ in 0..8 {
        g.process();
    }
    // After K outer ticks the LatchUp kernel will have written 0.7 into the
    // input field on every inner tick of every element, so each element's
    // process() (running 2x per outer tick) will have copied 0.7 to its
    // output.
    for i in 0..4 {
        let got = *g.latches[i].output;
        assert!(
            (got - 0.7).abs() < 1e-6,
            "latches[{i}].output = {got}, expected 0.7"
        );
    }
}

/// Trivial DC-emitting node: outputs constant `value` (set at construction).
/// Used to verify cross-rate fan-in sums correctly across N elements.
#[derive(Debug, Node)]
pub struct DcEmitter {
    pub output: StreamOutput,
    value: f32,
}

impl DcEmitter {
    pub fn new() -> Self {
        Self {
            output: StreamOutput::default(),
            value: 1.0,
        }
    }
}

impl Default for DcEmitter {
    fn default() -> Self {
        Self::new()
    }
}

impl SignalProcessor for DcEmitter {
    #[inline(always)]
    fn process(&mut self) {
        *self.output = self.value;
    }
}

graph! {
    name: FanInStreamArrayToScalar;
    output stream out;
    nodes {
        emitters = [DcEmitter::new(); 4] * 2;
    }
    connections {
        [sinc] emitters.output -> out;
    }
}

#[test]
fn fanin_stream_oversampled_array_to_outer_scalar() {
    let mut g = FanInStreamArrayToScalar::new();
    g.init(48_000.0);
    // Each emitter outputs 1.0; with 4 emitters fan-in sum = 4.0.
    // Run enough samples for the sinc downsampler to settle past its
    // group-delay transient.
    g.process_block(256);
    let written = &g.out_block[..256];
    // Look in the back half so the sinc kernel is past its warmup.
    let tail = &written[192..256];
    let avg: f32 = tail.iter().sum::<f32>() / tail.len() as f32;
    assert!(
        (avg - 4.0).abs() < 0.05,
        "expected fan-in sum ≈ 4.0 after sinc settles, got avg = {avg} over tail = {tail:?}"
    );
}
