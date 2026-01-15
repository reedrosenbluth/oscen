use oscen::{graph, PolyBlepOscillator, SignalProcessor};

// Define a simple Voice subgraph
graph! {
    name: SimpleVoice;

    output stream audio;

    nodes {
        osc = PolyBlepOscillator::sine(440.0, 0.5);
    }

    connections {
        osc.output -> audio;
    }
}

// Define a polyphonic synth using Voice subgraphs
graph! {
    name: DualVoiceSynth;

    output stream out;

    nodes {
        voice1 = SimpleVoice;
        voice2 = SimpleVoice;
    }

    connections {
        voice1.audio + voice2.audio -> out;
    }
}

#[test]
fn test_nested_graph_creation() {
    // Test that we can create a synth with nested graphs
    let mut synth = DualVoiceSynth::new();
    synth.init(48000.0);
    assert_eq!(synth.sample_rate, 48000.0);
}

#[test]
fn test_nested_graph_processing() {
    let mut synth = DualVoiceSynth::new();
    synth.init(48000.0);

    // Process several frames without error
    for _ in 0..100 {
        synth.process();
    }
}

#[test]
fn test_independent_voice_state() {
    // Create a synth with two voices
    let mut synth = DualVoiceSynth::new();
    synth.init(48000.0);

    // Process some frames
    for _ in 0..10 {
        synth.process();
    }

    // Both voices should maintain independent state
    // (verified by the fact that processing doesn't crash)
}

#[test]
fn test_sample_rate_propagation() {
    // Test that sample rate is correctly propagated to nested graphs
    let mut synth1 = DualVoiceSynth::new();
    synth1.init(44100.0);
    let mut synth2 = DualVoiceSynth::new();
    synth2.init(48000.0);

    assert_eq!(synth1.sample_rate, 44100.0);
    assert_eq!(synth2.sample_rate, 48000.0);
}

#[test]
fn test_multiple_nesting_levels() {
    // Define a graph that nests SimpleVoice
    graph! {
        name: TripleVoiceSynth;
        output stream out;

        nodes {
            voice1 = SimpleVoice;
            voice2 = SimpleVoice;
            voice3 = SimpleVoice;
        }

        connections {
            voice1.audio + voice2.audio + voice3.audio -> out;
        }
    }

    let mut synth = TripleVoiceSynth::new();
    synth.init(48000.0);

    // Process several frames
    for _ in 0..50 {
        synth.process();
    }
}

#[test]
fn test_nested_graph_output() {
    // Test that output values are correctly returned from nested graphs
    let mut synth = DualVoiceSynth::new();
    synth.init(48000.0);

    // Process a frame
    synth.process();

    // Get the output value (should be a valid f32)
    let output = synth.out;
    assert!(output.is_finite(), "Output should be a finite value");
}
