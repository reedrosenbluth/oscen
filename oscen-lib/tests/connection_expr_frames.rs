//! Typed frame connection expressions (Path 2a): a connection whose source is a
//! frame-valued expression — a frame constructor from scalars
//! (`Frame::<2>(a, b) -> stereoOut`) or a channel extraction by index
//! (`s.out[0] -> monoIn`) — typechecks and routes by its own/destination type,
//! while existing `f32` compound sources stay unchanged.
#![feature(inherent_associated_types)]

use float_cmp::approx_eq;
use oscen::prelude::*;
use oscen::Node;

const RATE: f32 = 48_000.0;

/// Emits a configurable constant `f32` on its stream output every sample.
#[derive(Debug, Node)]
pub struct ConstF32 {
    #[output(stream)]
    pub output: f32,
    pub val: f32,
}

impl ConstF32 {
    pub fn new(val: f32) -> Self {
        Self { output: 0.0, val }
    }
}

impl Default for ConstF32 {
    fn default() -> Self {
        Self::new(0.0)
    }
}

impl SignalProcessor for ConstF32 {
    #[inline(always)]
    fn process(&mut self) {
        self.output = self.val;
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

/// Latches the most recent `f32` it received on its stream input.
#[derive(Debug, Node)]
pub struct MonoSink {
    #[input(stream)]
    pub input: f32,
    pub last: f32,
}

impl MonoSink {
    pub fn new() -> Self {
        Self {
            input: 0.0,
            last: 0.0,
        }
    }
}

impl Default for MonoSink {
    fn default() -> Self {
        Self::new()
    }
}

impl SignalProcessor for MonoSink {
    #[inline(always)]
    fn process(&mut self) {
        self.last = self.input;
    }
}

// Merge: two f32 sources combined into a Frame<2> top-level output.
graph! {
    name: MergeGraph;

    output stream out: Frame<2>;

    nodes {
        a = ConstF32::new(0.25);
        b = ConstF32::new(-0.5);
    }

    connections {
        Frame::<2>(a.output, b.output) -> out;
    }
}

// Extract: each channel of a Frame<2> source routed to its own mono sink.
graph! {
    name: ExtractGraph;

    nodes {
        s = StereoConst::new();
        left = MonoSink::new();
        right = MonoSink::new();
    }

    connections {
        s.output[0] -> left.input;
        s.output[1] -> right.input;
    }
}

// Mono regression: an existing f32 compound source still works.
graph! {
    name: MonoCompoundGraph;

    output stream out;

    nodes {
        a = ConstF32::new(0.5);
        b = ConstF32::new(0.4);
    }

    connections {
        a.output * b.output -> out;
    }
}

#[test]
fn frame_constructor_merges_scalars_per_channel() {
    let mut graph = MergeGraph::new();
    graph.init(RATE);
    graph.process();
    assert!(
        approx_eq!(f32, graph.out.0[0], 0.25, epsilon = 1e-6)
            && approx_eq!(f32, graph.out.0[1], -0.5, epsilon = 1e-6),
        "expected Frame([0.25, -0.5]), got {:?}",
        graph.out
    );
}

#[test]
fn channel_extraction_routes_each_channel() {
    let mut graph = ExtractGraph::new();
    graph.init(RATE);
    graph.s.val = Frame([0.3, -0.7]);
    graph.process();
    assert!(
        approx_eq!(f32, graph.left.last, 0.3, epsilon = 1e-6),
        "channel 0: expected 0.3, got {}",
        graph.left.last
    );
    assert!(
        approx_eq!(f32, graph.right.last, -0.7, epsilon = 1e-6),
        "channel 1: expected -0.7, got {}",
        graph.right.last
    );
}

#[test]
fn mono_f32_compound_source_unchanged() {
    let mut graph = MonoCompoundGraph::new();
    graph.init(RATE);
    graph.process();
    assert!(
        approx_eq!(f32, graph.out, 0.2, epsilon = 1e-6),
        "expected 0.5 * 0.4 = 0.2, got {}",
        graph.out
    );
}
