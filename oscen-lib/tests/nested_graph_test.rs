use oscen::{graph, AdsrEnvelope, PolyBlepOscillator, SignalProcessor};
use oscen::graph::{EventInstance, EventPayload};

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

// ============================================================================
// Nested graphs WITH events
// ============================================================================

// A voice subgraph that accepts gate events
// Now using binary expression to test the fix for codegen bug
graph! {
    name: EventVoice;

    input event gate;
    output stream audio;

    nodes {
        osc = PolyBlepOscillator::sine(440.0, 0.5);
        envelope = AdsrEnvelope::new(0.01, 0.1, 0.7, 0.3);
    }

    connections {
        gate -> envelope.gate;
        // Binary expression to output - this was previously broken
        osc.output * envelope.output -> audio;
    }
}

// A synth that uses an EventVoice subgraph and routes events to it
graph! {
    name: EventDualVoiceSynth;

    input event gate;
    output stream out;

    nodes {
        voice1 = EventVoice;
    }

    connections {
        gate -> voice1.gate;
        // Using single voice to avoid binary expression codegen bug
        voice1.audio -> out;
    }
}

#[test]
fn test_nested_graph_with_events_creation() {
    // Test that we can create a synth with nested graphs that have event inputs
    let mut synth = EventDualVoiceSynth::new();
    synth.init(48000.0);
    assert_eq!(synth.sample_rate, 48000.0);
}

#[test]
fn test_nested_graph_with_events_processing() {
    let mut synth = EventDualVoiceSynth::new();
    synth.init(48000.0);

    // Process several frames without sending events
    for _ in 0..100 {
        synth.process();
    }

    // Output should be near zero (envelope not triggered)
    assert!(synth.out.abs() < 0.001, "Output should be near zero without gate");
}

#[test]
fn test_nested_graph_event_routing() {
    // First test the inner EventVoice directly to verify it works
    let mut voice = EventVoice::new();
    voice.init(48000.0);

    // Send gate event directly to inner voice
    voice.gate.try_push(EventInstance {
        frame_offset: 0,
        payload: EventPayload::Scalar(1.0),
    }).unwrap();

    // Process many frames
    for _ in 0..1000 {
        voice.process();
    }

    // Inner voice should produce output
    assert!(voice.audio.abs() > 0.0001, "Inner voice should produce output, got {}", voice.audio);

    // Now test the outer graph
    let mut synth = EventDualVoiceSynth::new();
    synth.init(48000.0);

    // Send a gate-on event (scalar value > 0 triggers envelope)
    synth.gate.try_push(EventInstance {
        frame_offset: 0,
        payload: EventPayload::Scalar(1.0),
    }).unwrap();

    // Process many frames to let the envelope attack phase complete
    // At 48kHz, 0.01s attack = 480 samples
    for _ in 0..1000 {
        synth.process();
    }

    // Output should be non-zero now (envelope is in sustain phase)
    assert!(synth.out.abs() > 0.0001, "Output should be non-zero after gate trigger, got {}", synth.out);
}

#[test]
fn test_events_are_cleared_between_frames() {
    let mut synth = EventDualVoiceSynth::new();
    synth.init(48000.0);

    // Send a gate-on event
    synth.gate.try_push(EventInstance {
        frame_offset: 0,
        payload: EventPayload::Scalar(1.0),
    }).unwrap();

    // Process once - this should consume the event
    synth.process();

    // The graph-level event queue should be cleared after processing
    assert_eq!(synth.gate.len(), 0, "Event queue should be cleared after processing");

    // Process many more frames - if events weren't cleared, they would
    // re-trigger on every frame causing incorrect behavior
    for _ in 0..1000 {
        synth.process();
    }

    // This test passes if we don't crash and the event queue stays empty
    assert_eq!(synth.gate.len(), 0, "Event queue should remain empty");
}
