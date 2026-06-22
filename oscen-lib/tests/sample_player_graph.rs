//! End-to-end test for the `external sample: AudioAsset;` binding into a
//! `SamplePlayer` (`sample -> player.buf`).
//!
//! The graph is silent before any load; after `graph.sample.load_wav(A)` it
//! loops buffer A; after a second `load_wav(B)` it swaps to looping buffer B.
//! Unlike `Convolver`, `SamplePlayer` swaps instantly with a hard playhead
//! reset, so no crossfade flush is needed.

use float_cmp::approx_eq;
use hound::{SampleFormat, WavSpec, WavWriter};
use oscen::prelude::*;

const RATE: u32 = 44_100;

graph! {
    name: PlayerGraph;

    output stream out;

    external sample: AudioAsset;

    nodes {
        player = SamplePlayer::new();
    }

    connections {
        sample -> player.buf;
        player.output -> out;
    }
}

/// Drive the graph one sample at a time and collect `out` for `n` samples.
fn run_graph(graph: &mut PlayerGraph, n: usize) -> Vec<f32> {
    (0..n)
        .map(|_| {
            graph.process();
            graph.out
        })
        .collect()
}

/// Write a mono 32-bit-float WAV at `RATE` for `samples`, into a unique temp
/// path tagged with the buffer name and process id.
fn write_mono_wav(samples: &[f32], tag: &str) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!(
        "oscen_sample_player_graph_{}_{}.wav",
        tag,
        std::process::id()
    ));
    let spec = WavSpec {
        channels: 1,
        sample_rate: RATE,
        bits_per_sample: 32,
        sample_format: SampleFormat::Float,
    };
    let mut writer = WavWriter::create(&path, spec).expect("create temp wav");
    for &s in samples {
        writer.write_sample(s).expect("write sample");
    }
    writer.finalize().expect("finalize temp wav");
    path
}

#[test]
fn silent_before_load_then_plays_then_swaps() {
    let a = vec![0.1f32, -0.2, 0.3, -0.4, 0.5];
    let b = vec![0.9f32, 0.8, 0.7];

    let path_a = write_mono_wav(&a, "a");
    let path_b = write_mono_wav(&b, "b");

    let mut graph = PlayerGraph::new();
    graph.init(RATE as f32);

    // Before any load: the player emits silence.
    let before = run_graph(&mut graph, 8);
    assert!(
        before.iter().all(|&y| y == 0.0),
        "graph must be silent before any asset is loaded, got {before:?}"
    );

    // Load buffer A and play it looped.
    graph.sample.set_graph_rate(RATE);
    graph.sample.load_wav(&path_a).expect("load_wav a");

    let out = run_graph(&mut graph, a.len() * 2);
    for (i, &expected) in a.iter().enumerate() {
        assert!(
            approx_eq!(f32, out[i], expected, epsilon = 1e-6),
            "buffer A at {i}: got {}, want {expected}",
            out[i]
        );
        assert!(
            approx_eq!(f32, out[i + a.len()], expected, epsilon = 1e-6),
            "buffer A loop at {}: got {}, want {expected}",
            i + a.len(),
            out[i + a.len()]
        );
    }

    // Swap to buffer B at runtime; the playhead resets and B loops.
    graph.sample.load_wav(&path_b).expect("load_wav b");

    let out2 = run_graph(&mut graph, b.len() * 2);
    for (i, &expected) in b.iter().enumerate() {
        assert!(
            approx_eq!(f32, out2[i], expected, epsilon = 1e-6),
            "buffer B at {i}: got {}, want {expected}",
            out2[i]
        );
        assert!(
            approx_eq!(f32, out2[i + b.len()], expected, epsilon = 1e-6),
            "buffer B loop at {}: got {}, want {expected}",
            i + b.len(),
            out2[i + b.len()]
        );
    }

    let _ = std::fs::remove_file(&path_a);
    let _ = std::fs::remove_file(&path_b);
}
