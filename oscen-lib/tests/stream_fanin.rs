//! Auto-summing stream fan-in: when two or more same-rate simple scalar/frame
//! stream sources connect to one stream destination, the generated `process()`
//! sums them (element-wise for `Frame<N>`) instead of letting the last edge
//! overwrite. A single source must stay an exact copy (no doubling).
#![feature(inherent_associated_types)]

use float_cmp::approx_eq;
use oscen::prelude::*;
use oscen::Node;

const RATE: f32 = 48_000.0;

/// Emits a constant `f32` on its stream output every sample.
#[derive(Debug, Node)]
pub struct ConstF32 {
    #[output(stream)]
    pub output: f32,
    val: f32,
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

/// Latches the most recent `f32` it received on its stream input.
#[derive(Debug, Node)]
pub struct SinkF32 {
    #[input(stream)]
    pub inp: f32,
    last: f32,
}

impl SinkF32 {
    pub fn new() -> Self {
        Self {
            inp: 0.0,
            last: 0.0,
        }
    }
}

impl Default for SinkF32 {
    fn default() -> Self {
        Self::new()
    }
}

impl SignalProcessor for SinkF32 {
    #[inline(always)]
    fn process(&mut self) {
        self.last = self.inp;
    }
}

/// Emits a constant `Frame<2>` on its stream output every sample.
#[derive(Debug, Node)]
pub struct ConstFrame2 {
    #[output(stream)]
    pub output: Frame<2>,
    val: Frame<2>,
}

impl ConstFrame2 {
    pub fn new(val: Frame<2>) -> Self {
        Self {
            output: Frame([0.0; 2]),
            val,
        }
    }
}

impl Default for ConstFrame2 {
    fn default() -> Self {
        Self::new(Frame([0.0; 2]))
    }
}

impl SignalProcessor for ConstFrame2 {
    #[inline(always)]
    fn process(&mut self) {
        self.output = self.val;
    }
}

/// Latches the most recent `Frame<2>` it received on its stream input.
#[derive(Debug, Node)]
pub struct SinkFrame2 {
    #[input(stream)]
    pub inp: Frame<2>,
    last: Frame<2>,
}

impl SinkFrame2 {
    pub fn new() -> Self {
        Self {
            inp: Frame([0.0; 2]),
            last: Frame([0.0; 2]),
        }
    }
}

impl Default for SinkFrame2 {
    fn default() -> Self {
        Self::new()
    }
}

impl SignalProcessor for SinkFrame2 {
    #[inline(always)]
    fn process(&mut self) {
        self.last = self.inp;
    }
}

graph! {
    name: MonoSumGraph;

    nodes {
        a = ConstF32::new(0.25);
        b = ConstF32::new(0.5);
        sink = SinkF32::new();
    }

    connections {
        a.output -> sink.inp;
        b.output -> sink.inp;
    }
}

graph! {
    name: StereoSumGraph;

    nodes {
        a = ConstFrame2::new(Frame([0.1, -0.2]));
        b = ConstFrame2::new(Frame([0.4, 0.7]));
        sink = SinkFrame2::new();
    }

    connections {
        a.output -> sink.inp;
        b.output -> sink.inp;
    }
}

graph! {
    name: SingleSourceGraph;

    nodes {
        a = ConstF32::new(0.3);
        sink = SinkF32::new();
    }

    connections {
        a.output -> sink.inp;
    }
}

graph! {
    name: MonoOutputSumGraph;

    output stream out;

    nodes {
        a = ConstF32::new(0.2);
        b = ConstF32::new(0.45);
    }

    connections {
        a.output -> out;
        b.output -> out;
    }
}

#[test]
fn mono_fanin_sums_sources() {
    let mut graph = MonoSumGraph::new();
    graph.init(RATE);
    graph.process();
    assert!(
        approx_eq!(f32, graph.sink.last, 0.75, epsilon = 1e-6),
        "expected 0.25 + 0.5 = 0.75, got {}",
        graph.sink.last
    );
}

#[test]
fn stereo_fanin_sums_per_channel() {
    let mut graph = StereoSumGraph::new();
    graph.init(RATE);
    graph.process();
    let got = graph.sink.last;
    assert!(
        approx_eq!(f32, got.0[0], 0.5, epsilon = 1e-6)
            && approx_eq!(f32, got.0[1], 0.5, epsilon = 1e-6),
        "expected Frame([0.1+0.4, -0.2+0.7]) = Frame([0.5, 0.5]), got {got:?}"
    );
}

#[test]
fn single_source_is_exact_copy() {
    let mut graph = SingleSourceGraph::new();
    graph.init(RATE);
    graph.process();
    assert!(
        approx_eq!(f32, graph.sink.last, 0.3, epsilon = 1e-6),
        "single source must not be doubled: expected 0.3, got {}",
        graph.sink.last
    );
}

#[test]
fn top_level_output_fanin_sums_sources() {
    let mut graph = MonoOutputSumGraph::new();
    graph.init(RATE);
    graph.process();
    assert!(
        approx_eq!(f32, graph.out, 0.65, epsilon = 1e-6),
        "expected 0.2 + 0.45 = 0.65, got {}",
        graph.out
    );
}
