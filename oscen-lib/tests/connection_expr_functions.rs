//! Named user functions applied on a connection: a connection whose source is a
//! call to an in-scope pure function, e.g. Cmajor's mid/side decode
//! `convert(ms) -> out`. Covers three shapes: frame -> frame, scalar -> scalar,
//! and multi-arg -> frame.
#![feature(inherent_associated_types)]

use float_cmp::approx_eq;
use oscen::prelude::*;
use oscen::Node;

const RATE: f32 = 48_000.0;

/// Pure mid/side -> left/right decode (Cmajor's documented example).
/// `v = [mid, side]` -> `[mid - side, mid + side]`.
fn decode_ms(v: Frame<2>) -> Frame<2> {
    Frame([v.0[0] - v.0[1], v.0[0] + v.0[1]])
}

/// Pure scalar function applied on a connection.
fn half(x: f32) -> f32 {
    x * 0.5
}

/// Pure multi-argument function: two scalar endpoints -> a frame.
fn merge2(l: f32, r: f32) -> Frame<2> {
    Frame([l, r])
}

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

// frame -> frame: a named function decodes a mid/side frame to a stereo output.
graph! {
    name: MsDecodeGraph;

    output stream out: Frame<2>;

    nodes {
        s = StereoConst::new();
    }

    connections {
        decode_ms(s.output) -> out;
    }
}

// scalar -> scalar: a named function applied to an f32 source.
graph! {
    name: ScalarFnGraph;

    output stream out;

    nodes {
        a = ConstF32::new(0.8);
    }

    connections {
        half(a.output) -> out;
    }
}

// multi-arg -> frame: a named function combines two scalar endpoints.
graph! {
    name: MultiArgGraph;

    output stream out: Frame<2>;

    nodes {
        a = ConstF32::new(0.25);
        b = ConstF32::new(-0.5);
    }

    connections {
        merge2(a.output, b.output) -> out;
    }
}

#[test]
fn named_function_decodes_frame_to_frame() {
    let mut graph = MsDecodeGraph::new();
    graph.init(RATE);
    // mid = 0.6, side = 0.1 -> left = 0.5, right = 0.7
    graph.s.val = Frame([0.6, 0.1]);
    graph.process();
    assert!(
        approx_eq!(f32, graph.out.0[0], 0.5, epsilon = 1e-6)
            && approx_eq!(f32, graph.out.0[1], 0.7, epsilon = 1e-6),
        "expected Frame([0.5, 0.7]), got {:?}",
        graph.out
    );
}

#[test]
fn named_function_applies_to_scalar() {
    let mut graph = ScalarFnGraph::new();
    graph.init(RATE);
    graph.process();
    assert!(
        approx_eq!(f32, graph.out, 0.4, epsilon = 1e-6),
        "expected half(0.8) = 0.4, got {}",
        graph.out
    );
}

#[test]
fn named_function_combines_multiple_endpoints() {
    let mut graph = MultiArgGraph::new();
    graph.init(RATE);
    graph.process();
    assert!(
        approx_eq!(f32, graph.out.0[0], 0.25, epsilon = 1e-6)
            && approx_eq!(f32, graph.out.0[1], -0.5, epsilon = 1e-6),
        "expected Frame([0.25, -0.5]), got {:?}",
        graph.out
    );
}
