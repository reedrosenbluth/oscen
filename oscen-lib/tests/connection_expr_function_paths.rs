//! Path-qualified function names on a connection (Gap A): a connection whose
//! source is a call to a path-qualified pure function, e.g.
//! `dsp::decode_ms(s.output) -> out`. Regression coverage keeps the bare-ident
//! call and the turbofish frame constructor parsing after the `Ident -> Path`
//! change.
#![feature(inherent_associated_types)]

use float_cmp::approx_eq;
use oscen::prelude::*;
use oscen::Node;

const RATE: f32 = 48_000.0;

mod dsp {
    use oscen::prelude::*;
    /// Pure mid/side -> left/right decode, behind a module path.
    pub fn decode_ms(v: Frame<2>) -> Frame<2> {
        Frame([v.0[0] - v.0[1], v.0[0] + v.0[1]])
    }
}

/// Bare (single-segment) mid/side decode for the bare-ident regression.
fn decode_ms(v: Frame<2>) -> Frame<2> {
    Frame([v.0[0] - v.0[1], v.0[0] + v.0[1]])
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

// path-qualified function: `dsp::decode_ms` applied on a connection.
graph! {
    name: PathFnGraph;

    output stream out: Frame<2>;

    nodes {
        s = StereoConst::new();
    }

    connections {
        dsp::decode_ms(s.output) -> out;
    }
}

// turbofish frame constructor regression: `Frame::<2>(a, b)` still parses.
graph! {
    name: TurbofishCtorGraph;

    output stream out: Frame<2>;

    nodes {
        a = ConstF32::new(0.25);
        b = ConstF32::new(-0.5);
    }

    connections {
        Frame::<2>(a.output, b.output) -> out;
    }
}

// path-qualified frame constructor: any call path ending in `Frame` is the
// frame constructor, so the qualified spelling behaves like the bare one.
graph! {
    name: QualifiedFrameCtorGraph;

    output stream out: Frame<2>;

    nodes {
        a = ConstF32::new(0.25);
        b = ConstF32::new(-0.5);
    }

    connections {
        oscen::frame::Frame::<2>(a.output, b.output) -> out;
    }
}

// bare-ident function regression: a single-segment name still works.
graph! {
    name: BareIdentFnGraph;

    output stream out: Frame<2>;

    nodes {
        s = StereoConst::new();
    }

    connections {
        decode_ms(s.output) -> out;
    }
}

#[test]
fn path_qualified_function_decodes_frame_to_frame() {
    let mut graph = PathFnGraph::new();
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
fn turbofish_frame_constructor_still_parses() {
    let mut graph = TurbofishCtorGraph::new();
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
fn qualified_frame_constructor_matches_bare() {
    let mut graph = QualifiedFrameCtorGraph::new();
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
fn bare_ident_function_still_works() {
    let mut graph = BareIdentFnGraph::new();
    graph.init(RATE);
    graph.s.val = Frame([0.6, 0.1]);
    graph.process();
    assert!(
        approx_eq!(f32, graph.out.0[0], 0.5, epsilon = 1e-6)
            && approx_eq!(f32, graph.out.0[1], 0.7, epsilon = 1e-6),
        "expected Frame([0.5, 0.7]), got {:?}",
        graph.out
    );
}
