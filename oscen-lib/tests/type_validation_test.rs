use oscen::{graph, PolyBlepOscillator, SignalProcessor, TptFilter};

// This test verifies that type validation catches common mistakes

// Test 1: Valid connections - should compile
graph! {
    name: ValidGraph;

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
fn test_valid_connections() {
    let mut graph = ValidGraph::new();
    graph.init(44100.0);
}

// Uncomment these to test error messages:
// They should produce helpful compile-time errors

/*
// Test 2: Using output() as destination - should fail
graph! {
    input value freq = 440.0;
    node osc = PolyBlepOscillator::saw(440.0, 0.6);

    connection {
        freq -> osc.output();  // ERROR: output() can't be a destination
    }
}
*/

/*
// Test 3: Using input() as source - should fail
graph! {
    node filter = TptFilter::new(1000.0, 0.707);
    node osc = PolyBlepOscillator::saw(440.0, 0.6);

    connection {
        filter.input() -> osc.frequency();  // ERROR: input() can't be a source
    }
}
*/
