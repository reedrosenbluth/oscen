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

// ----- Gap B: type-directed array broadcast --------------------------------

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

/// Latches an `f32` stream input every sample (scalar-broadcast sink).
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

// frame broadcast: a frame-returning function fans into a Frame<2>-input array.
graph! {
    name: FrameBroadcastGraph;

    nodes {
        s = StereoConst::new();
        sinks = [StereoSink::new(); 3];
    }

    connections {
        decode_ms(s.output) -> sinks.input;
    }
}

// f32 broadcast regression: a scalar function fans into an f32-input array.
graph! {
    name: F32BroadcastGraph;

    nodes {
        a = ConstF32::new(0.8);
        monosinks = [MonoSink::new(); 3];
    }

    connections {
        half(a.output) -> monosinks.input;
    }
}

// frame-constructor broadcast: a `Frame::<2>(...)` source fans into a
// Frame<2>-input array (formerly fenced off; now type-directed). Replaces the
// old `oscen-macros/tests/ui/frame_constructor_into_array.rs` compile-fail case.
graph! {
    name: FrameCtorBroadcastGraph;

    nodes {
        a = ConstF32::new(0.25);
        b = ConstF32::new(-0.5);
        sinks = [StereoSink::new(); 3];
    }

    connections {
        Frame::<2>(a.output, b.output) -> sinks.input;
    }
}

// numeric-literal broadcast regression: a compound source carrying a numeric
// literal (`a.output * 2.0`) fans into an f32 array. This exercises the literal
// inference the old `let __src: f32` pin protected — the projected-type binding
// must still coerce the literal under an f32 destination. (A *bare* literal
// source like `0.5 -> dest` produces no edge at all: `insert_edge` skips sources
// with no node reference, in any destination — pre-existing and out of scope for
// Gap B, so the regression uses a node-anchored literal-bearing source instead.)
graph! {
    name: LiteralBroadcastGraph;

    nodes {
        a = ConstF32::new(0.25);
        monosinks = [MonoSink::new(); 3];
    }

    connections {
        a.output * 2.0 -> monosinks.input;
    }
}

#[test]
fn frame_returning_function_broadcasts_into_node_array() {
    let mut graph = FrameBroadcastGraph::new();
    graph.init(RATE);
    // mid = 0.6, side = 0.1 -> left = 0.5, right = 0.7
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

#[test]
fn scalar_function_broadcasts_into_node_array() {
    let mut graph = F32BroadcastGraph::new();
    graph.init(RATE);
    graph.process();
    for i in 0..3 {
        assert!(
            approx_eq!(f32, graph.monosinks[i].last, 0.4, epsilon = 1e-6),
            "monosinks[{}].last expected half(0.8) = 0.4, got {}",
            i,
            graph.monosinks[i].last
        );
    }
}

#[test]
fn frame_constructor_broadcasts_into_node_array() {
    let mut graph = FrameCtorBroadcastGraph::new();
    graph.init(RATE);
    graph.process();
    for i in 0..3 {
        assert!(
            approx_eq!(f32, graph.sinks[i].last.0[0], 0.25, epsilon = 1e-6)
                && approx_eq!(f32, graph.sinks[i].last.0[1], -0.5, epsilon = 1e-6),
            "sinks[{}].last expected Frame([0.25, -0.5]), got {:?}",
            i,
            graph.sinks[i].last
        );
    }
}

#[test]
fn numeric_literal_broadcasts_into_node_array() {
    let mut graph = LiteralBroadcastGraph::new();
    graph.init(RATE);
    graph.process();
    for i in 0..3 {
        assert!(
            approx_eq!(f32, graph.monosinks[i].last, 0.5, epsilon = 1e-6),
            "monosinks[{}].last expected 0.5, got {}",
            i,
            graph.monosinks[i].last
        );
    }
}
