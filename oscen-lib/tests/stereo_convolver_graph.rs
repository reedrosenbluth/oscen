//! End-to-end test for a stereo `Convolver<Frame<2>>` driven through a `graph!`
//! with an `external ir: AudioAsset;` binding.
//!
//! A controllable `Frame<2>` source feeds the convolver's stereo input and an
//! internal `Frame<2>` sink captures the output (top-level `output stream` is
//! `f32`-only, so multi-channel signals ride internal edges — the
//! `frame_streams.rs` pattern). The graph is silent before the IR loads; after
//! publishing a known **2-channel** IR (distinct L/R), each output channel
//! matches a direct time-domain convolution of that channel's input with that
//! channel's IR — proving per-channel convolution with no L↔R bleed.
#![feature(inherent_associated_types)]

use float_cmp::approx_eq;
use hound::{SampleFormat, WavSpec, WavWriter};
use oscen::convolution::Convolver;
use oscen::prelude::*;
use oscen::Node;

const RATE: u32 = 44_100;

/// Emits a settable stereo frame each tick (the graph's controllable input).
#[derive(Debug, Node)]
pub struct StereoSource {
    #[output(stream)]
    pub out: Frame<2>,
    value: Frame<2>,
}

impl StereoSource {
    pub fn new() -> Self {
        Self {
            out: Frame([0.0; 2]),
            value: Frame([0.0; 2]),
        }
    }
}

impl Default for StereoSource {
    fn default() -> Self {
        Self::new()
    }
}

impl SignalProcessor for StereoSource {
    #[inline(always)]
    fn process(&mut self) {
        self.out = self.value;
    }
}

/// Captures the most recent stereo frame on a `Frame<2>` input.
#[derive(Debug, Node)]
pub struct StereoSink {
    #[input(stream)]
    pub inp: Frame<2>,
    last: Frame<2>,
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
    name: StereoConvGraph;

    external ir: AudioAsset;

    nodes {
        source = StereoSource::new();
        conv = Convolver::<Frame<2>>::new();
        sink = StereoSink::new();
    }

    connections {
        source.out -> conv.input;
        conv.output -> sink.inp;
        ir -> conv.ir;
    }
}

/// Deterministic pseudo-noise in [-1, 1] (LCG; no rand dependency). Same
/// generator as the other convolution tests.
fn noise(len: usize, seed: u64) -> Vec<f32> {
    let mut state = seed
        .wrapping_mul(2862933555777941757)
        .wrapping_add(3037000493);
    (0..len)
        .map(|_| {
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            ((state >> 33) as f32 / (u32::MAX >> 1) as f32) - 1.0
        })
        .collect()
}

/// Direct time-domain convolution: `out[n] = sum_k ir[k] * input[n - k]`.
fn convolve(input: &[f32], ir: &[f32]) -> Vec<f32> {
    (0..input.len())
        .map(|n| {
            let mut acc = 0.0f32;
            for k in 0..ir.len().min(n + 1) {
                acc += ir[k] * input[n - k];
            }
            acc
        })
        .collect()
}

/// Compare two signals with a tolerance relative to the reference's peak (an
/// FFT round-trip vs direct-convolution tolerance, matching `asset_graph.rs`).
fn assert_close_rel(got: &[f32], want: &[f32], label: &str) {
    assert_eq!(got.len(), want.len(), "{label}: length mismatch");
    let scale = want.iter().fold(1.0f32, |m, &w| m.max(w.abs()));
    for (i, (&g, &w)) in got.iter().zip(want.iter()).enumerate() {
        assert!(
            approx_eq!(f32, g, w, epsilon = 1e-3 * scale),
            "{label}: index {i}: got {g}, want {w}"
        );
    }
}

/// Write a 2-channel 32-bit-float WAV at `RATE` (interleaved L,R,L,R,...).
fn write_stereo_ir_wav(left: &[f32], right: &[f32]) -> std::path::PathBuf {
    assert_eq!(left.len(), right.len());
    let path =
        std::env::temp_dir().join(format!("oscen_stereo_conv_ir_{}.wav", std::process::id()));
    let spec = WavSpec {
        channels: 2,
        sample_rate: RATE,
        bits_per_sample: 32,
        sample_format: SampleFormat::Float,
    };
    let mut writer = WavWriter::create(&path, spec).expect("create temp IR wav");
    for (&l, &r) in left.iter().zip(right.iter()) {
        writer.write_sample(l).expect("write L");
        writer.write_sample(r).expect("write R");
    }
    writer.finalize().expect("finalize temp IR wav");
    path
}

#[test]
fn stereo_convolver_graph_silent_then_per_channel_matches_reference() {
    // Distinct per-channel IRs (different seeds) so a broadcast or L↔R-bleed bug
    // cannot pass. Length 700 engages all three Gardner tiers.
    let ir_l = noise(700, 1);
    let ir_r = noise(700, 2);
    let path = write_stereo_ir_wav(&ir_l, &ir_r);

    let mut graph = StereoConvGraph::new();
    graph.init(RATE as f32);

    // Before any load: even with input fed, both channels are silent.
    let pre_l = noise(64, 3);
    let pre_r = noise(64, 4);
    for (i, (&l, &r)) in pre_l.iter().zip(&pre_r).enumerate() {
        graph.source.value = Frame([l, r]);
        graph.process();
        assert_eq!(
            graph.sink.last,
            Frame([0.0, 0.0]),
            "graph must be silent before the IR loads (sample {i})"
        );
    }

    // Load the 2-channel IR and fade it fully in with silence; the engine state
    // settles back to zero so a subsequent input convolves from a clean start.
    graph.ir.set_graph_rate(RATE);
    graph.ir.load_wav(&path).expect("load_wav");
    for _ in 0..4096 {
        graph.source.value = Frame([0.0, 0.0]);
        graph.process();
    }

    // Feed distinct per-channel noise; capture per-channel output.
    let in_l = noise(2000, 5);
    let in_r = noise(2000, 6);
    let mut out_l = Vec::with_capacity(in_l.len());
    let mut out_r = Vec::with_capacity(in_r.len());
    for (&l, &r) in in_l.iter().zip(&in_r) {
        graph.source.value = Frame([l, r]);
        graph.process();
        out_l.push(graph.sink.last.0[0]);
        out_r.push(graph.sink.last.0[1]);
    }

    // Each channel must match the direct convolution of its own input with its
    // own IR — per-channel convolution, no cross-channel bleed.
    let ref_l = convolve(&in_l, &ir_l);
    let ref_r = convolve(&in_r, &ir_r);
    assert_close_rel(&out_l, &ref_l, "left channel == conv(in_L, IR_L)");
    assert_close_rel(&out_r, &ref_r, "right channel == conv(in_R, IR_R)");

    let _ = std::fs::remove_file(&path);
}
