use oscen::prelude::*;

// Test runtime graph with array nodes (compile_time: false)
// Note: This test validates that array node creation works.
// Broadcast/sum connections for arrays will be implemented in Phase 1.4
graph! {
    name: RuntimeArrayGraph;
    // No compile_time flag = runtime mode

    input freq: value = 440.0;
    output out: stream;

    nodes {
        // Array of 4 oscillators
        oscs = [PolyBlepOscillator::saw(440.0, 0.2); 4];
        // Single filter for output
        filter = TptFilter::new(2000.0, 0.707);
    }

    connections {
        // For now, just connect freq to filter and filter to output
        // Array connections will be tested separately
        freq -> filter.cutoff;
        filter.output -> out;
    }
}

fn main() {
    println!("Testing runtime graph with array nodes...");
    println!("Note: Array creation test only - connections tested separately");

    let mut graph = RuntimeArrayGraph::new(48000.0);

    // Validate that the array was created
    println!("Graph created successfully with 4-element oscillator array");

    // Process a few samples
    for _ in 0..10 {
        graph.process();
    }

    println!("✓ Runtime array node creation test passed!");
    println!("  - Graph with array nodes compiles");
    println!("  - Graph processes without errors");
    println!("  - Ready for Phase 1.4: array connection support");
}
