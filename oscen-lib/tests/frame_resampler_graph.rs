//! End-to-end Frame<2> cross-rate edge through the full `graph!` path.
//!
//! A constant stereo frame is upsampled (outer→inner, factor 2) into an inner
//! `* 2` passthrough node and downsampled back (inner→outer) into a sink. With
//! the linear policy and a DC input, each channel round-trips to its constant
//! after warmup — proving the resampler state, codegen buffers, and
//! ConnectEndpoints transfers all carry Frame<2> as a unit (no crosstalk, no
//! zeroing, no broadcast). Node-to-node edges make the CrossRateKernel
//! projection fire (graph-level endpoints would fall back to the concrete f32
//! kernel and not test the frame path).
#![feature(inherent_associated_types)]

use oscen::{graph, Frame, Node, SignalProcessor};

#[derive(Debug, Node)]
pub struct StereoConstSrc {
    #[output(stream)]
    pub out: Frame<2>,
    value: Frame<2>,
}

impl StereoConstSrc {
    pub fn new(value: Frame<2>) -> Self {
        Self {
            out: Frame([0.0; 2]),
            value,
        }
    }
}

impl Default for StereoConstSrc {
    fn default() -> Self {
        Self::new(Frame([0.0; 2]))
    }
}

impl SignalProcessor for StereoConstSrc {
    #[inline(always)]
    fn process(&mut self) {
        self.out = self.value;
    }
}

#[derive(Debug, Node)]
pub struct StereoPass {
    #[input(stream)]
    pub inp: Frame<2>,
    #[output(stream)]
    pub out: Frame<2>,
}

impl StereoPass {
    pub fn new() -> Self {
        Self {
            inp: Frame([0.0; 2]),
            out: Frame([0.0; 2]),
        }
    }
}

impl Default for StereoPass {
    fn default() -> Self {
        Self::new()
    }
}

impl SignalProcessor for StereoPass {
    #[inline(always)]
    fn process(&mut self) {
        self.out = self.inp;
    }
}

#[derive(Debug, Node)]
pub struct StereoSink {
    #[input(stream)]
    pub inp: Frame<2>,
    pub last: Frame<2>,
}

impl StereoSink {
    pub fn new() -> Self {
        Self {
            inp: Frame([0.0; 2]),
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
        self.last = self.inp;
    }
}

graph! {
    name: StereoCrossRate;
    nodes {
        src = StereoConstSrc::new(Frame([0.3, -0.7]));
        inner = StereoPass::new() * 2;
        sink = StereoSink::new();
    }
    connections {
        [linear] src.out -> inner.inp;
        [linear] inner.out -> sink.inp;
    }
}

#[test]
fn frame2_round_trips_through_cross_rate_edges_per_channel() {
    let mut g = StereoCrossRate::new();
    g.init(48_000.0);
    // Warm up: linear up/down settle a DC input to the exact constant within a
    // few outer ticks. 16 is comfortable margin.
    g.process_block(16);
    let last = g.sink.last;
    // Distinct per-channel values rule out broadcast/zeroing/crosstalk bugs.
    assert!(
        (last.0[0] - 0.3).abs() < 1e-4,
        "L channel did not round-trip: {}",
        last.0[0]
    );
    assert!(
        (last.0[1] - (-0.7)).abs() < 1e-4,
        "R channel did not round-trip: {}",
        last.0[1]
    );
}
