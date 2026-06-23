//! Combined path: a *path-qualified*, frame-returning connection function
//! broadcast into a *node array* — `dsp::decode_ms(s.output) -> sinks.input;`.
//! Gap A (path-qualified function names) and Gap B (type-directed array
//! broadcast) each have coverage, but their intersection did not. This locks it.
#![feature(inherent_associated_types)]

use float_cmp::approx_eq;
use oscen::prelude::*;
use oscen::Node;

const RATE: f32 = 48_000.0;

mod dsp {
    use oscen::prelude::*;
    /// Pure mid/side -> left/right decode, behind a module path.
    /// `v = [mid, side]` -> `[mid - side, mid + side]`.
    pub fn decode_ms(v: Frame<2>) -> Frame<2> {
        Frame([v.0[0] - v.0[1], v.0[0] + v.0[1]])
    }
}

/// Emits a configurable constant `Frame<2>` on its stream output every sample.
#[derive(Debug, Node)]
pub struct StereoConst {
    #[output(stream)]
    pub output: Frame<2>,
    pub val: Frame<2>,
}

impl StereoConst {
    pub fn new() -> Self {
        Self {
            output: Frame([0.0; 2]),
            val: Frame([0.0; 2]),
        }
    }
}

impl Default for StereoConst {
    fn default() -> Self {
        Self::new()
    }
}

impl SignalProcessor for StereoConst {
    #[inline(always)]
    fn process(&mut self) {
        self.output = self.val;
    }
}

/// Latches a `Frame<2>` stream input every sample (frame-broadcast sink).
#[derive(Debug, Node)]
pub struct StereoSink {
    #[input(stream)]
    pub input: Frame<2>,
    pub last: Frame<2>,
}

impl StereoSink {
    pub fn new() -> Self {
        Self {
            input: Frame([0.0; 2]),
            last: Frame([0.0; 2]),
        }
    }
}

impl Default for StereoSink {
    fn default() -> Self {
        Self::new()
    }
}

impl SignalProcessor for StereoSink {
    #[inline(always)]
    fn process(&mut self) {
        self.last = self.input;
    }
}

// path-qualified frame function broadcast into a Frame<2>-input node array.
graph! {
    name: PathFnBroadcastGraph;

    nodes {
        s = StereoConst::new();
        sinks = [StereoSink::new(); 3];
    }

    connections {
        dsp::decode_ms(s.output) -> sinks.input;
    }
}

#[test]
fn path_qualified_function_broadcasts_into_node_array() {
    let mut graph = PathFnBroadcastGraph::new();
    graph.init(RATE);
    // mid = 0.6, side = 0.1 -> left = 0.5, right = 0.7, latched on every element.
    graph.s.val = Frame([0.6, 0.1]);
    graph.process();
    for i in 0..3 {
        assert!(
            approx_eq!(f32, graph.sinks[i].last.0[0], 0.5, epsilon = 1e-6)
                && approx_eq!(f32, graph.sinks[i].last.0[1], 0.7, epsilon = 1e-6),
            "sinks[{}].last expected Frame([0.5, 0.7]), got {:?}",
            i,
            graph.sinks[i].last
        );
    }
}
