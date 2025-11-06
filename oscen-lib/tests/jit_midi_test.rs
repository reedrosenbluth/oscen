// Test JIT with MIDI events (simulating electric-piano flow)

use oscen::{MidiParser, MidiVoiceHandler, queue_raw_midi};
use oscen::graph::Graph;
use oscen::graph::jit::{CraneliftJit, GraphStateBuilder};
use slotmap::Key;

#[test]
fn test_jit_midi_events() {
    let sample_rate = 44100.0;

    println!("\n=== Testing MIDI with Interpreted Mode ===");
    let mut graph_interp = Graph::new(sample_rate);

    let midi_parser = graph_interp.add_node(MidiParser::new());
    let voice_handler = graph_interp.add_node(MidiVoiceHandler::new());

    // Connect MIDI parser to voice handler
    graph_interp.connect(midi_parser.note_on, voice_handler.note_on);
    graph_interp.connect(midi_parser.note_off, voice_handler.note_off);

    // Send MIDI note on: Note 60 (middle C), velocity 100
    let note_on_bytes = [0x90, 60, 100];
    queue_raw_midi(
        &mut graph_interp,
        midi_parser.midi_in,
        0,
        &note_on_bytes,
    );

    // Process several frames to let the note trigger
    let mut interpreted_outputs = Vec::new();
    for i in 0..10 {
        graph_interp.process().expect("Process failed");

        // Get the frequency output from voice handler
        let freq = graph_interp.get_value(&voice_handler.frequency).unwrap_or(0.0);
        let gate = graph_interp.get_value(&voice_handler.gate).unwrap_or(0.0);
        println!("Interpreted Frame {}: freq={:.2}, gate={:.2}", i, freq, gate);
        interpreted_outputs.push((freq, gate));
    }

    println!("\n=== Testing MIDI with JIT Mode ===");
    let mut graph_jit = Graph::new(sample_rate);

    let midi_parser2 = graph_jit.add_node(MidiParser::new());
    let voice_handler2 = graph_jit.add_node(MidiVoiceHandler::new());

    // Connect MIDI parser to voice handler
    graph_jit.connect(midi_parser2.note_on, voice_handler2.note_on);
    graph_jit.connect(midi_parser2.note_off, voice_handler2.note_off);

    // Send the same MIDI note on
    queue_raw_midi(
        &mut graph_jit,
        midi_parser2.midi_in,
        0,
        &note_on_bytes,
    );

    let ir = graph_jit.to_ir().expect("Failed to extract IR");
    println!("Extracted IR with {} nodes", ir.nodes.len());

    let mut jit = CraneliftJit::new().expect("Failed to create JIT");
    let compiled = jit.compile(&ir).expect("Failed to compile");
    println!("Successfully compiled graph");

    let mut state_builder = GraphStateBuilder::new(&ir, &mut graph_jit.nodes);

    let mut jit_outputs = Vec::new();
    for i in 0..10 {
        // Process ramps before JIT
        graph_jit.process_ramps();

        let (mut state, _temps) = state_builder.build(
            &mut graph_jit.nodes,
            &mut graph_jit.endpoints,
        );

        compiled.process(&mut state);

        // Get the frequency output from voice handler
        let freq = graph_jit.get_value(&voice_handler2.frequency).unwrap_or(0.0);
        let gate = graph_jit.get_value(&voice_handler2.gate).unwrap_or(0.0);
        println!("JIT Frame {}: freq={:.2}, gate={:.2}", i, freq, gate);
        jit_outputs.push((freq, gate));
    }

    println!("\n=== Comparing Outputs ===");
    let interp_non_zero = interpreted_outputs.iter().filter(|&&(f, g)| f > 0.1 || g > 0.1).count();
    let jit_non_zero = jit_outputs.iter().filter(|&&(f, g)| f > 0.1 || g > 0.1).count();

    println!("Interpreted: {} non-zero frames out of 10", interp_non_zero);
    println!("JIT: {} non-zero frames out of 10", jit_non_zero);

    // After sending a note-on MIDI message, we should see non-zero output
    assert!(interp_non_zero > 0, "Interpreted mode should produce non-zero output after MIDI note-on");
    assert!(jit_non_zero > 0, "JIT mode should produce non-zero output after MIDI note-on");

    // The outputs should be similar
    for (i, ((interp_f, interp_g), (jit_f, jit_g))) in interpreted_outputs.iter().zip(jit_outputs.iter()).enumerate() {
        let freq_diff = (interp_f - jit_f).abs();
        let gate_diff = (interp_g - jit_g).abs();
        println!("Frame {}: freq diff={:.6}, gate diff={:.6}", i, freq_diff, gate_diff);

        // Allow small floating point differences
        if interp_f.abs() > 0.1 || jit_f.abs() > 0.1 {
            assert!(freq_diff < 1.0,
                "Frame {} frequency differs too much: interp={}, jit={}", i, interp_f, jit_f);
        }
        if interp_g.abs() > 0.1 || jit_g.abs() > 0.1 {
            assert!(gate_diff < 0.1,
                "Frame {} gate differs too much: interp={}, jit={}", i, interp_g, jit_g);
        }
    }
}
