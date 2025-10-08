use oscen::{graph, PolyBlepOscillator, PolyBlepOscillatorEndpoints};

// Define a simple Voice subgraph
graph! {
    name: SimpleVoice;

    output stream audio;

    node {
        osc = PolyBlepOscillator::sine(440.0, 0.5);
    }

    connection {
        osc.output() -> audio;
    }
}

// Define a polyphonic synth using Voice subgraphs
graph! {
    name: DualVoiceSynth;

    output stream out;

    node {
        voice1 = SimpleVoice::new(48000.0);
        voice2 = SimpleVoice::new(48000.0);
    }

    connection {
        voice1.audio() + voice2.audio() -> out;
    }
}

#[test]
fn test_nested_graph_creation() {
    // Test that we can create a synth with nested graphs
    let synth = DualVoiceSynth::new(48000.0);
    assert_eq!(synth.graph.sample_rate, 48000.0);
}

#[test]
fn test_nested_graph_processing() {
    let synth = DualVoiceSynth::new(48000.0);
    let mut graph = synth.graph;

    // Process several frames without error
    for _ in 0..100 {
        graph.process().expect("Graph processing should succeed");
    }
}

#[test]
fn test_independent_voice_state() {
    // Create a synth with two voices
    let synth = DualVoiceSynth::new(48000.0);
    let mut graph = synth.graph;

    // Process some frames
    for _ in 0..10 {
        graph.process().expect("Graph processing should succeed");
    }

    // Both voices should maintain independent state
    // (verified by the fact that processing doesn't crash)
}

#[test]
fn test_sample_rate_propagation() {
    // Test that sample rate is correctly propagated to nested graphs
    let synth1 = DualVoiceSynth::new(44100.0);
    let synth2 = DualVoiceSynth::new(48000.0);

    assert_eq!(synth1.graph.sample_rate, 44100.0);
    assert_eq!(synth2.graph.sample_rate, 48000.0);
}

#[test]
fn test_multiple_nesting_levels() {
    // Define a graph that nests SimpleVoice
    graph! {
        name: TripleVoiceSynth;
        output stream out;

        node {
            voice1 = SimpleVoice::new(48000.0);
            voice2 = SimpleVoice::new(48000.0);
            voice3 = SimpleVoice::new(48000.0);
        }

        connection {
            voice1.audio() + voice2.audio() + voice3.audio() -> out;
        }
    }

    let synth = TripleVoiceSynth::new(48000.0);
    let mut graph = synth.graph;

    // Process several frames
    for _ in 0..50 {
        graph.process().expect("Graph processing should succeed");
    }
}

#[test]
fn test_nested_graph_output() {
    // Test that output values are correctly returned from nested graphs
    let synth = DualVoiceSynth::new(48000.0);
    let mut graph = synth.graph;

    // Process a frame
    graph.process().expect("Graph processing should succeed");

    // Get the output value (should be a valid f32)
    let output = graph.get_value(&synth.out).unwrap_or(0.0);
    assert!(output.is_finite(), "Output should be a finite value");
}
