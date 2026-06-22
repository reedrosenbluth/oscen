//! End-to-end test for the `external <name>: <Type>;` asset binding in the
//! `graph!` macro (asset wiring sub-project 4c).
//!
//! A graph declares an `external ir: AudioAsset;` bound into a `Convolver`'s
//! `asset` input (`ir -> reverb.ir`). Before any load the graph is silent;
//! after `graph.ir.load_wav(...)` the convolver reproduces the loaded IR,
//! matching a standalone `Convolver::with_ir`.

use float_cmp::approx_eq;
use hound::{SampleFormat, WavSpec, WavWriter};
use oscen::convolution::Convolver;
use oscen::prelude::*;
use oscen::SignalProcessor;

const RATE: u32 = 44_100;

graph! {
    name: AssetReverbGraph;

    input stream dry;
    output stream wet;

    external ir: AudioAsset;

    nodes {
        reverb = Convolver::new();
    }

    connections {
        dry -> reverb.input;
        reverb.output -> wet;
        ir -> reverb.ir;
    }
}

/// Deterministic pseudo-noise in [-1, 1] (LCG; no rand dependency). Same
/// generator as `tests/convolution.rs`.
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

/// Drive the graph one sample at a time and collect `wet`.
fn run_graph(graph: &mut AssetReverbGraph, input: &[f32]) -> Vec<f32> {
    input
        .iter()
        .map(|&x| {
            graph.dry = x;
            graph.process();
            graph.wet
        })
        .collect()
}

/// Standalone convolver impulse response (fresh state), for comparison.
fn standalone_response(ir: &[f32], n: usize) -> Vec<f32> {
    let mut node = Convolver::with_ir(ir.to_vec());
    node.set_sample_rate(RATE as f32);
    node.prepare();
    let mut input = vec![0.0f32; n];
    input[0] = 1.0;
    input
        .iter()
        .map(|&x| {
            node.input = x;
            node.process();
            node.output
        })
        .collect()
}

/// Write a mono 32-bit-float WAV at `RATE` for `ir`.
fn write_ir_wav(ir: &[f32]) -> std::path::PathBuf {
    let path =
        std::env::temp_dir().join(format!("oscen_asset_graph_ir_{}.wav", std::process::id()));
    let spec = WavSpec {
        channels: 1,
        sample_rate: RATE,
        bits_per_sample: 32,
        sample_format: SampleFormat::Float,
    };
    let mut writer = WavWriter::create(&path, spec).expect("create temp IR wav");
    for &s in ir {
        writer.write_sample(s).expect("write sample");
    }
    writer.finalize().expect("finalize temp IR wav");
    path
}

#[test]
fn asset_graph_silent_before_load_then_reproduces_ir() {
    let ir = noise(700, 12);
    let ir_path = write_ir_wav(&ir);

    let mut graph = AssetReverbGraph::new();
    graph.init(RATE as f32);

    // Before any load: feed a unit impulse; the graph must be silent.
    let mut impulse = vec![0.0f32; 1024];
    impulse[0] = 1.0;
    let before = run_graph(&mut graph, &impulse);
    assert!(
        before.iter().all(|&y| y == 0.0),
        "graph must be silent before any asset is loaded"
    );

    // Load the IR at the graph rate and publish it to the convolver.
    graph.ir.set_graph_rate(RATE);
    graph.ir.load_wav(&ir_path).expect("load_wav");

    // Flush the equal-power crossfade with zeros so the newly published engine
    // is fully faded in (the crossfade is ~20 ms; 4096 zero-samples is ample).
    let _ = run_graph(&mut graph, &vec![0.0f32; 4096]);

    // Now feed an impulse: the warmed engine reproduces the IR with zero
    // latency, matching a standalone `Convolver::with_ir`.
    let after = run_graph(&mut graph, &impulse);
    let standalone = standalone_response(&ir, impulse.len());

    let scale = ir.iter().fold(1.0f32, |m, &h| m.max(h.abs()));
    let eps = 1e-3 * scale;

    // Zero latency: the first post-impulse sample is ir[0].
    assert!(
        approx_eq!(f32, after[0], ir[0], epsilon = eps),
        "first sample: got {}, want ir[0]={}",
        after[0],
        ir[0]
    );

    for (i, (&g, &h)) in after.iter().zip(ir.iter()).enumerate() {
        assert!(
            approx_eq!(f32, g, h, epsilon = eps),
            "graph output vs IR at {i}: got {g}, want {h}"
        );
    }

    for (i, (&g, &s)) in after.iter().zip(standalone.iter()).enumerate() {
        assert!(
            approx_eq!(f32, g, s, epsilon = eps),
            "graph output vs standalone with_ir at {i}: got {g}, want {s}"
        );
    }

    let _ = std::fs::remove_file(&ir_path);
}
