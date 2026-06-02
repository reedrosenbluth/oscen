//! Same-rate `Frame<N>` stream edges: passthrough and array fan-in summing.
//!
//! These tests prove a multi-channel `Frame<N>` value can travel one SAME-RATE
//! stream edge as a unit, and that an array of `Frame<N>` sources can fan into
//! a single `StreamInput<Frame<N>>` by summing element-wise — with NO resampler
//! involvement (every edge is same-rate).
//!
//! - Passthrough (`StereoConst.out -> sink.inp`) lowers to
//!   `ConnectEndpoints<StreamOutput<T>, StreamInput<T>>`, which is already
//!   generic over `T: Copy`, so `Frame<2>` rides through with no codegen change.
//! - Array fan-in lowers to
//!   `self.sink.inp = self.srcs.iter().map(|n| n.out).sum()`
//!   (see `oscen-graph-compiler/src/codegen/emit_edge.rs::emit_fanin_connect`).
//!   With a raw-`Frame<2>` output field the per-element `map` yields `Frame<2>`
//!   and the destination is `StreamInput<Frame<2>>`, so summation selects
//!   `impl Sum<Frame<N>> for StreamInput<Frame<N>>`. This mirrors the existing
//!   raw-`f32`-output fan-in idiom in `tests/array_fanin_stream_input.rs`.
#![feature(inherent_associated_types)]

use oscen::graph::{StreamInput, StreamOutput};
use oscen::{graph, Frame, Node, SignalProcessor};

/// Emits a constant stereo frame every tick on a typed `StreamOutput<Frame<2>>`.
/// Used for the passthrough test: a `StreamOutput<Frame<2>> -> StreamInput<Frame<2>>`
/// edge resolves via the generic `ConnectEndpoints<StreamOutput<T>, StreamInput<T>>`.
#[derive(Debug, Node)]
pub struct StereoConst {
    pub out: StreamOutput<Frame<2>>,
    value: Frame<2>,
}

impl StereoConst {
    pub fn new(value: Frame<2>) -> Self {
        Self {
            out: StreamOutput(Frame([0.0; 2])),
            value,
        }
    }
}

impl Default for StereoConst {
    fn default() -> Self {
        Self::new(Frame([0.0; 2]))
    }
}

impl SignalProcessor for StereoConst {
    #[inline(always)]
    fn process(&mut self) {
        *self.out = self.value;
    }
}

/// Voice element for the fan-in test: emits its configured stereo frame on a
/// raw-`Frame<2>` stream output. The raw output field (not the `StreamOutput`
/// wrapper) is what makes the per-element `map` in the fan-in yield `Frame<2>`
/// directly, mirroring the raw-`f32` fan-in idiom in
/// `tests/array_fanin_stream_input.rs`.
#[derive(Debug, Node)]
pub struct StereoVoice {
    #[output(stream)]
    pub out: Frame<2>,
    value: Frame<2>,
}

impl StereoVoice {
    pub fn new(value: Frame<2>) -> Self {
        Self {
            out: Frame([0.0; 2]),
            value,
        }
    }
}

impl Default for StereoVoice {
    fn default() -> Self {
        Self::new(Frame([0.0; 2]))
    }
}

impl SignalProcessor for StereoVoice {
    #[inline(always)]
    fn process(&mut self) {
        self.out = self.value;
    }
}

/// Captures the most recent stereo frame it received on a typed
/// `StreamInput<Frame<2>>`.
#[derive(Debug, Node)]
pub struct StereoSink {
    pub inp: StreamInput<Frame<2>>,
    last: Frame<2>,
}

impl StereoSink {
    pub fn new() -> Self {
        Self {
            inp: StreamInput(Frame([0.0; 2])),
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
        self.last = *self.inp;
    }
}

graph! {
    name: StereoPassthrough;
    nodes {
        src = StereoConst::new(Frame([0.3, -0.7]));
        sink = StereoSink::new();
    }
    connections {
        src.out -> sink.inp;
    }
}

#[test]
fn stereo_passthrough_carries_both_channels() {
    let mut g = StereoPassthrough::new();
    g.init(48_000.0);
    g.process();
    // The whole Frame<2> rode the edge as a unit: both channels preserved,
    // and distinct per-channel values rule out a broadcast/zeroing bug.
    assert_eq!(g.sink.last, Frame([0.3, -0.7]));
}

graph! {
    name: StereoFanIn;
    nodes {
        srcs = [StereoVoice::new(Frame([0.0; 2])); 2];
        sink = StereoSink::new();
    }
    connections {
        srcs.out -> sink.inp;
    }
}

#[test]
fn stereo_fan_in_sums_per_channel() {
    let mut g = StereoFanIn::new();
    g.init(48_000.0);

    // Distinct per-element, per-channel values so the assertion distinguishes a
    // true element-wise sum (Frame([0.3, 3.0])) from a broadcast or
    // last-writer-wins (which would land on Frame([0.2, 2.0])).
    g.srcs[0].value = Frame([0.1, 1.0]);
    g.srcs[1].value = Frame([0.2, 2.0]);

    g.process();

    // Two stereo contributions summed element-wise into one StreamInput<Frame<2>>.
    assert_eq!(g.sink.last, Frame([0.3, 3.0]));
}
