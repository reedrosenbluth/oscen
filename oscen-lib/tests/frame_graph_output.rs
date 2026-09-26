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

/// Duplicates a mono input into both channels of a stereo frame.
#[derive(Debug, Node)]
pub struct Widen {
    #[input(stream)]
    pub input: f32,
    #[output(stream)]
    pub output: Frame<2>,
}

impl Widen {
    pub fn new() -> Self {
        Self {
            input: 0.0,
            output: Frame([0.0, 0.0]),
        }
    }
}

impl SignalProcessor for Widen {
    #[inline(always)]
    fn process(&mut self) {
        self.output = Frame([self.input, self.input]);
    }
}

// Mono stream input, stereo stream output: no single `BlockRender<F>` exists,
// so the graph omits it instead of failing to compile.
graph! {
    name: MonoToStereoGraph;

    input stream dry;
    output stream wet: Frame<2>;

    nodes {
        widen = Widen::new();
    }

    connections {
        dry -> widen.input;
        widen.output -> wet;
    }
}

#[test]
fn mixed_frame_graph_builds_and_processes() {
    let mut graph = MonoToStereoGraph::new();
    graph.init(48_000.0);

    graph.dry = 0.5;
    graph.process();
    assert_eq!(graph.wet.0, [0.5, 0.5]);
    assert_eq!(graph.get_stream_output(0).map(|f| f.0), Some([0.5, 0.5]));

    for (i, s) in graph.dry_block[..4].iter_mut().enumerate() {
        *s = i as f32;
    }
    graph.process_block(4);
    let rendered: Vec<[f32; 2]> = graph.wet_block[..4].iter().map(|f| f.0).collect();
    assert_eq!(rendered, [[0.0, 0.0], [1.0, 1.0], [2.0, 2.0], [3.0, 3.0]]);
}
