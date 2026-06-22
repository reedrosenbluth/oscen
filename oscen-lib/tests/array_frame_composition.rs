//! Array × frame composition + frame-typed top-level output regression.
//!
//! Proves the two multiplicity axes compose: a **node array** of **frame-valued**
//! sources (vector width `Frame<2>`) fans into one frame bus and is **summed**
//! per channel (`FanoutShape::FanIn` + `Frame<N>: Sum`). Also locks the
//! already-shipped frame-typed top-level `output stream out: Frame<2>;` against
//! regression — the `sample_player` example depends on it.
#![feature(inherent_associated_types)]

use float_cmp::approx_eq;
use oscen::prelude::*;
use oscen::Node;

const RATE: f32 = 48_000.0;

/// Emits a configurable constant `Frame<2>` on its stream output every sample.
/// `val` is public so array elements can be given distinct per-voice frames.
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

graph! {
    name: ArrayStereoSumGraph;

    output stream out: Frame<2>;

    nodes {
        voices = [StereoConst::new(); 3];
    }

    connections {
        voices.output -> out;
    }
}

graph! {
    name: SingleStereoGraph;

    output stream out: Frame<2>;

    nodes {
        src = StereoConst::new();
    }

    connections {
        src.output -> out;
    }
}

fn assert_frame_close(got: Frame<2>, want: Frame<2>, label: &str) {
    assert!(
        approx_eq!(f32, got.0[0], want.0[0], epsilon = 1e-6)
            && approx_eq!(f32, got.0[1], want.0[1], epsilon = 1e-6),
        "{label}: got {got:?}, want {want:?}"
    );
}

#[test]
fn array_of_stereo_voices_fans_in_summed_per_channel() {
    let mut graph = ArrayStereoSumGraph::new();
    graph.init(RATE);

    // Distinct per-voice frames so a broadcast/last-write bug cannot pass.
    graph.voices[0].val = Frame([0.1, -0.2]);
    graph.voices[1].val = Frame([0.4, 0.5]);
    graph.voices[2].val = Frame([-0.05, 0.7]);

    graph.process();

    // Per-channel sum: [0.1 + 0.4 - 0.05, -0.2 + 0.5 + 0.7] = [0.45, 1.0].
    assert_frame_close(graph.out, Frame([0.45, 1.0]), "array stereo fan-in sum");
}

#[test]
fn frame_typed_top_level_output_reproduces_each_channel() {
    let mut graph = SingleStereoGraph::new();
    graph.init(RATE);

    // Distinct L/R rules out a broadcast or channel-zeroing bug.
    graph.src.val = Frame([0.25, -0.5]);

    graph.process();

    assert_frame_close(graph.out, Frame([0.25, -0.5]), "single frame output");
}
