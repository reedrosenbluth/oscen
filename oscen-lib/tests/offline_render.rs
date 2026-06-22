use oscen::graph::BlockRender;
use oscen::prelude::*;

graph! {
    name: GainGraph;

    input stream audio_in;
    output stream audio_out;

    nodes {
        gain = Gain::new(0.5);
    }

    connections {
        audio_in -> gain.input;
        gain.output -> audio_out;
    }
}

#[test]
fn generated_graph_renders_offline() {
    let mut g = GainGraph::new();
    g.init(48_000.0);

    // Longer than one block to prove the internal chunk loop.
    let len = 600;
    let input: Vec<f32> = (0..len).map(|i| (i % 5) as f32).collect();

    let out = g.render_mono(&input, 0);

    assert_eq!(out.len(), len);
    for (i, &v) in out.iter().enumerate() {
        assert!(
            (v - (i % 5) as f32 * 0.5).abs() < 1e-6,
            "mismatch at frame {i}: got {v}"
        );
    }
}

#[test]
fn generated_graph_reports_stream_counts() {
    assert_eq!(GainGraph::NUM_STREAM_INPUTS, 1);
    assert_eq!(GainGraph::NUM_STREAM_OUTPUTS, 1);
}
