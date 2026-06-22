//! End-to-end test for a stereo `SamplePlayer<Frame<2>>` driven through a
//! `graph!`: an `external sample: AudioAsset;` feeds `player.buf`, and the
//! player's `Frame<2>` output flows to an internal stereo sink (top-level
//! `output stream` is `f32`-only, so multi-channel output rides an internal
//! edge — the `frame_streams.rs` pattern).
//!
//! The graph is silent before any load; after `graph.sample.load_wav(A)` it
//! loops buffer A, reproducing each channel exactly. Distinct L/R values rule
//! out a broadcast or channel-zeroing bug.
#![feature(inherent_associated_types)]

use float_cmp::approx_eq;
use hound::{SampleFormat, WavSpec, WavWriter};
use oscen::prelude::*;
use oscen::Node;

const RATE: u32 = 44_100;

/// Captures the most recent stereo frame it received on a `Frame<2>` input.
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
    name: StereoPlayerGraph;

    external sample: AudioAsset;

    nodes {
        player = SamplePlayer::<Frame<2>>::new();
        sink = StereoSink::new();
    }

    connections {
        sample -> player.buf;
        player.output -> sink.inp;
    }
}

/// Drive the graph one sample at a time and collect `sink.last` for `n` samples.
fn run_graph(graph: &mut StereoPlayerGraph, n: usize) -> Vec<Frame<2>> {
    (0..n)
        .map(|_| {
            graph.process();
            graph.sink.last
        })
        .collect()
}

/// Write a stereo 32-bit-float WAV at `RATE` for the given per-channel buffers
/// (interleaved L,R,L,R,...), into a unique temp path tagged with `tag` + pid.
fn write_stereo_wav(left: &[f32], right: &[f32], tag: &str) -> std::path::PathBuf {
    assert_eq!(left.len(), right.len());
    let path = std::env::temp_dir().join(format!(
        "oscen_stereo_sample_player_graph_{}_{}.wav",
        tag,
        std::process::id()
    ));
    let spec = WavSpec {
        channels: 2,
        sample_rate: RATE,
        bits_per_sample: 32,
        sample_format: SampleFormat::Float,
    };
    let mut writer = WavWriter::create(&path, spec).expect("create temp wav");
    for (&l, &r) in left.iter().zip(right.iter()) {
        writer.write_sample(l).expect("write L");
        writer.write_sample(r).expect("write R");
    }
    writer.finalize().expect("finalize temp wav");
    path
}

fn assert_frame_close(got: Frame<2>, want: Frame<2>, label: &str) {
    assert!(
        approx_eq!(f32, got.0[0], want.0[0], epsilon = 1e-6)
            && approx_eq!(f32, got.0[1], want.0[1], epsilon = 1e-6),
        "{label}: got {got:?}, want {want:?}"
    );
}

#[test]
fn silent_before_load_then_reproduces_each_channel() {
    // Distinct L/R so a broadcast/zeroing bug cannot pass.
    let left = [0.1f32, -0.2, 0.3, -0.4];
    let right = [-0.1f32, 0.2, -0.3, 0.4];
    let path = write_stereo_wav(&left, &right, "a");

    let mut graph = StereoPlayerGraph::new();
    graph.init(RATE as f32);

    // Before any load: the player emits silence on both channels.
    let before = run_graph(&mut graph, 8);
    for (i, f) in before.iter().enumerate() {
        assert_frame_close(*f, Frame([0.0, 0.0]), &format!("silent before load at {i}"));
    }

    // Load the stereo buffer and play it looped.
    graph.sample.set_graph_rate(RATE);
    graph.sample.load_wav(&path).expect("load_wav");

    // Two full loops: exact per-channel reproduction, including wrap-around.
    let out = run_graph(&mut graph, left.len() * 2);
    for (i, (&l, &r)) in left.iter().zip(right.iter()).enumerate() {
        assert_frame_close(out[i], Frame([l, r]), &format!("frame {i}"));
        assert_frame_close(
            out[i + left.len()],
            Frame([l, r]),
            &format!("loop frame {i}"),
        );
    }

    let _ = std::fs::remove_file(&path);
}
