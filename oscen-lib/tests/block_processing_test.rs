use oscen::{graph, oscillators::PolyBlepOscillator, SignalProcessor};

// ============================================================================
// Test 1: Stream-only graph — process_block(N) == N × process()
// ============================================================================

graph! {
    name: StreamOnlyGraph;

    input value freq = 440.0;
    output stream audio_out;

    nodes {
        osc = PolyBlepOscillator::saw(440.0, 0.6);
    }

    connections {
        freq -> osc.frequency_mod;
        osc.output -> audio_out;
    }
}

#[test]
fn test_block_equals_per_sample_stream_only() {
    let block_size = 64;

    // Run per-sample
    let mut graph_a = StreamOnlyGraph::new();
    graph_a.init(44100.0);
    let mut per_sample_outputs = Vec::with_capacity(block_size);
    for _ in 0..block_size {
        graph_a.process();
        per_sample_outputs.push(graph_a.audio_out);
    }

    // Run block
    let mut graph_b = StreamOnlyGraph::new();
    graph_b.init(44100.0);
    graph_b.process_block(block_size);

    // Compare sample-by-sample
    for i in 0..block_size {
        assert_eq!(
            graph_b.audio_out_block[i], per_sample_outputs[i],
            "Mismatch at sample {}: block={} per_sample={}",
            i, graph_b.audio_out_block[i], per_sample_outputs[i]
        );
    }
}

#[test]
fn test_block_max_block_size_constant() {
    assert_eq!(StreamOnlyGraph::MAX_BLOCK_SIZE, 512);
}

// ============================================================================
// Test 2: Graph with ramped value inputs
// ============================================================================

graph! {
    name: RampedBlockGraph;

    input value freq = 440.0 [ramp: 100];
    output stream audio_out;

    nodes {
        osc = PolyBlepOscillator::saw(440.0, 0.6);
    }

    connections {
        freq -> osc.frequency_mod;
        osc.output -> audio_out;
    }
}

#[test]
fn test_block_with_ramped_value() {
    let block_size = 128;

    // Run per-sample with ramp
    let mut graph_a = RampedBlockGraph::new();
    graph_a.init(44100.0);
    graph_a.set_freq(880.0);
    let mut per_sample_outputs = Vec::with_capacity(block_size);
    for _ in 0..block_size {
        graph_a.process();
        per_sample_outputs.push(graph_a.audio_out);
    }

    // Run block with same ramp
    let mut graph_b = RampedBlockGraph::new();
    graph_b.init(44100.0);
    graph_b.set_freq(880.0);
    graph_b.process_block(block_size);

    // Compare
    for i in 0..block_size {
        assert_eq!(
            graph_b.audio_out_block[i], per_sample_outputs[i],
            "Ramp mismatch at sample {}: block={} per_sample={}",
            i, graph_b.audio_out_block[i], per_sample_outputs[i]
        );
    }

    // Ramp should have completed (100 frames < 128 block size)
    assert!(!graph_b.freq.is_ramping());
    assert_eq!(graph_b.freq.current, 880.0);
}

// ============================================================================
// Test 3: Graph with event inputs — sub-block splitting
// ============================================================================

use oscen::prelude::*;

graph! {
    name: EventBlockGraph;

    input midi_in: event;
    output stream audio_out;
    output note_on_out: event;

    nodes {
        midi_parser = MidiParser::new();
        voice_handler = MidiVoiceHandler::new();
    }

    connections {
        midi_in -> midi_parser.midi_in;
        midi_parser.note_on -> voice_handler.note_on;
        midi_parser.note_off -> voice_handler.note_off;
        voice_handler.frequency -> audio_out;
    }
}

#[test]
fn test_block_with_events_at_frame_zero() {
    use oscen::graph::{EventInstance, EventPayload};
    use oscen::midi::RawMidiMessage;

    let block_size = 32;

    // Push a note-on event at frame 0
    let note_on_bytes = [0x90, 69, 100]; // Note A4, velocity 100
    let msg = RawMidiMessage::new(&note_on_bytes);

    // Per-sample: push event, process N times
    let mut graph_a = EventBlockGraph::new();
    graph_a.init(44100.0);
    let _ = graph_a.midi_in.try_push(EventInstance {
        frame_offset: 0,
        payload: EventPayload::Object(std::sync::Arc::new(msg)),
    });
    let mut per_sample_outputs = Vec::with_capacity(block_size);
    for _ in 0..block_size {
        graph_a.process();
        per_sample_outputs.push(graph_a.audio_out);
    }

    // Block: push event with frame_offset 0, process_block
    let msg2 = RawMidiMessage::new(&note_on_bytes);
    let mut graph_b = EventBlockGraph::new();
    graph_b.init(44100.0);
    let _ = graph_b.midi_in.try_push(EventInstance {
        frame_offset: 0,
        payload: EventPayload::Object(std::sync::Arc::new(msg2)),
    });
    graph_b.process_block(block_size);

    // Compare
    for i in 0..block_size {
        assert_eq!(
            graph_b.audio_out_block[i], per_sample_outputs[i],
            "Event mismatch at sample {}: block={} per_sample={}",
            i, graph_b.audio_out_block[i], per_sample_outputs[i]
        );
    }

    // Frequency should be set to A4 (440 Hz)
    assert!((graph_b.voice_handler.frequency - 440.0).abs() < 0.01);
}

#[test]
fn test_block_with_events_at_mid_block() {
    use oscen::graph::{EventInstance, EventPayload};
    use oscen::midi::RawMidiMessage;

    let block_size = 32;
    let event_frame = 16;

    let note_on_bytes = [0x90, 69, 100]; // Note A4

    // Per-sample: push event at the right sample, process N times
    let mut graph_a = EventBlockGraph::new();
    graph_a.init(44100.0);
    let mut per_sample_outputs = Vec::with_capacity(block_size);
    for i in 0..block_size {
        if i == event_frame {
            let msg = RawMidiMessage::new(&note_on_bytes);
            let _ = graph_a.midi_in.try_push(EventInstance {
                frame_offset: 0,
                payload: EventPayload::Object(std::sync::Arc::new(msg)),
            });
        }
        graph_a.process();
        per_sample_outputs.push(graph_a.audio_out);
    }

    // Block: push event with frame_offset = event_frame, process_block
    let msg2 = RawMidiMessage::new(&note_on_bytes);
    let mut graph_b = EventBlockGraph::new();
    graph_b.init(44100.0);
    let _ = graph_b.midi_in.try_push(EventInstance {
        frame_offset: event_frame as u32,
        payload: EventPayload::Object(std::sync::Arc::new(msg2)),
    });
    graph_b.process_block(block_size);

    // Compare — before event_frame, output should be identical (both 440.0 default freq)
    // After event_frame, output should also match (both see the note-on)
    for i in 0..block_size {
        assert_eq!(
            graph_b.audio_out_block[i], per_sample_outputs[i],
            "Mid-block event mismatch at sample {}: block={} per_sample={}",
            i, graph_b.audio_out_block[i], per_sample_outputs[i]
        );
    }
}

#[test]
fn test_block_with_multiple_events_different_frames() {
    use oscen::graph::{EventInstance, EventPayload};
    use oscen::midi::RawMidiMessage;

    let block_size = 32;

    let note_a4 = [0x90, 69, 100]; // A4 at frame 5
    let note_c5 = [0x90, 72, 100]; // C5 at frame 20

    // Per-sample
    let mut graph_a = EventBlockGraph::new();
    graph_a.init(44100.0);
    let mut per_sample_outputs = Vec::with_capacity(block_size);
    for i in 0..block_size {
        if i == 5 {
            let msg = RawMidiMessage::new(&note_a4);
            let _ = graph_a.midi_in.try_push(EventInstance {
                frame_offset: 0,
                payload: EventPayload::Object(std::sync::Arc::new(msg)),
            });
        }
        if i == 20 {
            let msg = RawMidiMessage::new(&note_c5);
            let _ = graph_a.midi_in.try_push(EventInstance {
                frame_offset: 0,
                payload: EventPayload::Object(std::sync::Arc::new(msg)),
            });
        }
        graph_a.process();
        per_sample_outputs.push(graph_a.audio_out);
    }

    // Block: push both events with correct frame_offsets
    let mut graph_b = EventBlockGraph::new();
    graph_b.init(44100.0);
    let msg_a4 = RawMidiMessage::new(&note_a4);
    let msg_c5 = RawMidiMessage::new(&note_c5);
    let _ = graph_b.midi_in.try_push(EventInstance {
        frame_offset: 5,
        payload: EventPayload::Object(std::sync::Arc::new(msg_a4)),
    });
    let _ = graph_b.midi_in.try_push(EventInstance {
        frame_offset: 20,
        payload: EventPayload::Object(std::sync::Arc::new(msg_c5)),
    });
    graph_b.process_block(block_size);

    // Compare
    for i in 0..block_size {
        assert_eq!(
            graph_b.audio_out_block[i], per_sample_outputs[i],
            "Multi-event mismatch at sample {}: block={} per_sample={}",
            i, graph_b.audio_out_block[i], per_sample_outputs[i]
        );
    }
}

// ============================================================================
// Test 4: Empty block (0 frames)
// ============================================================================

#[test]
fn test_block_zero_frames() {
    let mut graph = StreamOnlyGraph::new();
    graph.init(44100.0);
    // Should not panic
    graph.process_block(0);
}

// ============================================================================
// Test 5: Stream input block buffer
// ============================================================================

graph! {
    name: StreamInputGraph;

    input stream audio_in;
    output stream audio_out;

    nodes {
        gain = oscen::Gain::new(0.5);
    }

    connections {
        audio_in -> gain.input;
        gain.output -> audio_out;
    }
}

#[test]
fn test_block_with_stream_input() {
    let block_size = 16;

    // Fill input block buffer
    let mut graph = StreamInputGraph::new();
    graph.init(44100.0);
    for i in 0..block_size {
        graph.audio_in_block[i] = i as f32 * 0.1;
    }
    graph.process_block(block_size);

    // Verify outputs are input * gain (0.5)
    for i in 0..block_size {
        let expected = i as f32 * 0.1 * 0.5;
        assert!(
            (graph.audio_out_block[i] - expected).abs() < 1e-6,
            "Stream input mismatch at sample {}: got={} expected={}",
            i, graph.audio_out_block[i], expected
        );
    }
}
