use oscen::{graph, PolyBlepOscillator, TptFilter};

graph! {
    name: SimpleGraph;
    compile_time: true;

    input value freq = 440.0;

    node osc = PolyBlepOscillator::saw(440.0, 1.0);
    node filter = TptFilter::new(1000.0, 0.7);

    connections {
        osc.output -> filter.input;
    }
}

fn main() {
    let mut graph = SimpleGraph::new(44100.0);
    println!("Created static graph with compile_time: true");
    println!("sample_rate: {}", graph.sample_rate);
    println!("\nProcessing first 10 samples:");

    for i in 0..10 {
        let output = graph.process();
        println!("  Sample {}: {:.6}", i, output);
    }

    println!("\nFilter output after processing:");
    println!("  filter.output: {:.6}", graph.filter.output);
}
