// This example intentionally creates a cycle to test compile-time cycle detection
// It should FAIL to compile with a cycle detection error

use oscen::{graph, PolyBlepOscillator, TptFilter};

graph! {
    name: CyclicGraph;
    compile_time: true;

    node osc = PolyBlepOscillator::saw(440.0, 1.0);
    node filter = TptFilter::new(1000.0, 0.7);

    connections {
        // Create a cycle: osc -> filter -> osc
        osc.output -> filter.input;
        filter.output -> osc.frequency_mod;  // This creates a cycle!
    }
}

fn main() {
    let mut graph = CyclicGraph::new(44100.0);
    println!("This should not compile!");
}
