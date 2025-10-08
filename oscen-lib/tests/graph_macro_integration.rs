use oscen::{
    filters::tpt::TptFilterEndpoints, graph, PolyBlepOscillator, PolyBlepOscillatorEndpoints,
    TptFilter,
};

// Define the graph at module level
graph! {
    name: SimpleGraph;

    input value freq = 440.0;

    node {
        osc = PolyBlepOscillator::saw(440.0, 0.6);
        filter = TptFilter::new(1000.0, 0.707);
    }

    connection {
        freq -> osc.frequency();
        osc.output() -> filter.input();
    }
}

#[test]
fn test_simple_graph_macro() {
    // Create instance and verify it compiles
    let _ctx = SimpleGraph::new(44100.0);
    // Just verify it compiles for now
}
