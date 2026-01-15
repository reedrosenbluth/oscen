use oscen::{graph, PolyBlepOscillator, SignalProcessor, TptFilter};

// Test that comma-separated parameter specs parse correctly
// Before the fix, [log, ramp(100)] would fail with "unexpected token, expected `]`"
// This is the EXACT same as graph_macro_integration but with param specs added
graph! {
    name: TestGraph;

    input value freq = 440.0 [log, ramp(100)];

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
fn test_param_specs_with_commas() {
    // The main achievement: this test compiles!
    // Before the fix, the parser would choke on commas in param specs like [log, ramp(100)]
    let mut graph = TestGraph::new();
    graph.init(44100.0);
}
