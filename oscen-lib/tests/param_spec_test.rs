use oscen::{graph, PolyBlepOscillator, SignalProcessor, TptFilter};

// Test that comma-separated parameter specs parse correctly with ramp annotation
graph! {
    name: TestGraph;

    input value freq = 440.0 [log, ramp: 100];

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
    // Before the fix, the parser would choke on commas in param specs like [log, ramp: 100]
    let mut graph = TestGraph::new();
    graph.init(44100.0);

    // Test the ramped value input has setter methods
    graph.set_freq(880.0);  // Uses default 100-frame ramp
    graph.set_freq_with_ramp(440.0, 50);  // Custom 50-frame ramp
    graph.set_freq_immediate(1000.0);  // Immediate change (no ramp)

    // The freq field should be a ValueRampState
    assert_eq!(graph.freq.current, 1000.0);
}
