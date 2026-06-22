//! Regression test for frame-typed top-level stream output: declaring a graph
//! output as `output stream out: Frame<2>;` and reading `graph.out` as a
//! `Frame<2>` after `process()`. This support is threaded through the compiler;
//! this test locks it so it cannot silently regress.
#![feature(inherent_associated_types)]

use float_cmp::approx_eq;
use oscen::prelude::*;
use oscen::Node;

/// Emits a constant, distinct-per-channel stereo frame every sample.
#[derive(Debug, Node)]
pub struct StereoConst {
    #[output(stream)]
    pub output: Frame<2>,
}

impl StereoConst {
    pub fn new() -> Self {
        Self {
            output: Frame([0.25, -0.5]),
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
        self.output = Frame([0.25, -0.5]);
    }
}

graph! {
    name: FrameOutputGraph;

    output stream out: Frame<2>;

    nodes {
        src = StereoConst::new();
    }

    connections {
        src.output -> out;
    }
}

#[test]
fn frame_typed_top_level_output_reads_per_channel() {
    let mut graph = FrameOutputGraph::new();
    graph.init(48_000.0);
    graph.process();

    assert!(
        approx_eq!(f32, graph.out.0[0], 0.25, epsilon = 1e-6),
        "channel 0: got {}, want 0.25",
        graph.out.0[0]
    );
    assert!(
        approx_eq!(f32, graph.out.0[1], -0.5, epsilon = 1e-6),
        "channel 1: got {}, want -0.5",
        graph.out.0[1]
    );
}
