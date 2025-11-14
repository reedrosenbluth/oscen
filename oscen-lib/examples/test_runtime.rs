use oscen::filters::tpt::TptFilterEndpoints;
use oscen::oscillators::PolyBlepOscillatorEndpoints;
use oscen::{graph, PolyBlepOscillator, TptFilter};

graph! {
    name: RuntimeGraph;
    compile_time: false;

    input value freq = 440.0;

    node osc = PolyBlepOscillator::saw(440.0, 1.0);
    node filter = TptFilter::new(1000.0, 0.7);

    connections {
        osc.output -> filter.input;
    }
}

fn main() {
    let mut graph = RuntimeGraph::new(44100.0);
    println!("Created runtime graph: {:?}", graph);
    println!(
        "Has graph field: {}",
        std::mem::size_of_val(&graph.graph) > 0
    );

    // Process a sample to verify it works
    let _ = graph.graph.process();
    println!("Processed one sample successfully");
}
