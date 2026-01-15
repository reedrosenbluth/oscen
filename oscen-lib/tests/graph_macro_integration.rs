use oscen::{graph, PolyBlepOscillator, SignalProcessor, TptFilter};

// Define the graph at module level
graph! {
    name: SimpleGraph;

    input value freq = 440.0;

    nodes {
        osc = PolyBlepOscillator::saw(440.0, 0.6);
        filter = TptFilter::new(1000.0, 0.707);
    }

    connections {
        // Use frequency_mod (public stream input) instead of frequency (private value input)
        freq -> osc.frequency_mod;
        osc.output -> filter.input;
    }
}

#[test]
fn test_simple_graph_macro() {
    // Create instance and verify it compiles
    let mut graph = SimpleGraph::new();
    graph.init(44100.0);
    // Just verify it compiles for now
}
