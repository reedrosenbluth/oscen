use oscen::{graph, PolyBlepOscillator, SignalProcessor, TptFilter};

graph! {
    name: SimpleGraph;

    input value freq = 440.0;

    nodes {
        osc = PolyBlepOscillator::saw(440.0, 1.0);
        filter = TptFilter::new(1000.0, 0.7);
    }

    connections {
        osc.output -> filter.input;
    }
}

fn main() {
    let mut graph = SimpleGraph::new();
    graph.init(44100.0);
    println!("Created static graph");
    println!("sample_rate: {}", graph.sample_rate);
    println!("\nProcessing first 10 samples:");

    for i in 0..10 {
        graph.process();
        println!("  Sample {}: {:.6}", i, graph.filter.output);
    }

    println!("\nFilter output after processing:");
    println!("  filter.output: {:.6}", graph.filter.output);
}
