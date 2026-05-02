//! Cross-rate fan-out integration tests for rate-annotated array nodes.
//! Exercises the four shapes (Scalar / Broadcast / FanIn / Parallel) across
//! stream / value / event endpoints over a rate boundary.

use oscen::graph::{ValueInput, ValueOutput};
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
