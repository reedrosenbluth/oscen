use oscen::{graph, PolyBlepOscillator, PolyBlepOscillatorEndpoints};

// Define a Voice subgraph with note and audio endpoints
graph! {
    name: Voice;

    input event note;
    output stream audio;

    node {
        osc = PolyBlepOscillator::sine(440.0, 0.5);
    }

    connection {
        osc.output() -> audio;
    }
}

// Use Voice as a node in a polyphonic synth
graph! {
    name: PolySynth;

    output stream out;

    node {
        voice1 = Voice::new(48000.0);
        voice2 = Voice::new(48000.0);
    }

    connection {
        voice1.audio() + voice2.audio() -> out;
    }
}

fn main() {
    let synth = PolySynth::new(48000.0);

    // Test that graph processes without error
    let mut graph = synth.graph;
    for _ in 0..100 {
        if let Err(e) = graph.process() {
            eprintln!("Error processing graph: {}", e);
            return;
        }
    }

    println!("Nested graph test passed!");
}
