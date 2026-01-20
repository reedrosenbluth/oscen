mod add_value;
mod crossfade;
mod fm_operator;
mod midi_input;
mod mixer;
mod pivot_voice;
mod vca;

use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use oscen::prelude::*;
use slint::ComponentHandle;

use midi_input::MidiConnection;

slint::include_modules!();

#[derive(Clone, Copy, Debug)]
enum ParamChange {
    // OP3 parameters
    Op3Ratio(f32),
    Op3Level(f32),
    Op3Feedback(f32),
    Op3Attack(f32),
    Op3Decay(f32),
    Op3Sustain(f32),
    Op3Release(f32),
    // OP2 parameters
    Op2Ratio(f32),
    Op2Level(f32),
    Op2Feedback(f32),
    Op2Attack(f32),
    Op2Decay(f32),
    Op2Sustain(f32),
    Op2Release(f32),
    // OP1 envelope parameters
    Op1Attack(f32),
    Op1Decay(f32),
    Op1Sustain(f32),
    Op1Release(f32),
    // Route: blends OP3 between OP2 (0.0) and OP1 (1.0)
    Route(f32),
    // Filter parameters
    Cutoff(f32),
    Resonance(f32),
    FilterAttack(f32),
    FilterDecay(f32),
    FilterSustain(f32),
    FilterRelease(f32),
    FilterEnvAmount(f32),
}

// Main polyphonic Pivot synth with 8 voices
graph! {
    name: PivotGraph;

    // MIDI input (raw MIDI bytes)
    input midi_in: event;

    // OP3 parameters
    input op3_ratio: value = 3.0;
    input op3_level: value = 0.5;
    input op3_feedback: value = 0.0;
    input op3_attack: value = 0.01;
    input op3_decay: value = 0.1;
    input op3_sustain: value = 0.7;
    input op3_release: value = 0.3;

    // OP2 parameters
    input op2_ratio: value = 2.0;
    input op2_level: value = 0.5;
    input op2_feedback: value = 0.0;
    input op2_attack: value = 0.01;
    input op2_decay: value = 0.1;
    input op2_sustain: value = 0.7;
    input op2_release: value = 0.3;

    // OP1 parameters (ratio always 1.0, no feedback)
    input op1_ratio: value = 1.0;
    input op1_attack: value = 0.01;
    input op1_decay: value = 0.2;
    input op1_sustain: value = 0.8;
    input op1_release: value = 0.5;

    // Route: blends OP3 between OP2 (0.0) and OP1 (1.0)
    input route: value = 0.0;

    // Filter parameters
    input cutoff: value = 2000.0;
    input resonance: value = 0.707;
    input filter_attack: value = 0.01;
    input filter_decay: value = 0.2;
    input filter_sustain: value = 0.5;
    input filter_release: value = 0.3;
    input filter_env_amount: value = 0.0;

    output audio_out: stream;

    nodes {
        midi_parser = MidiParser::new();
        voice_allocator = VoiceAllocator::<8>::new();
        voice_handlers = [MidiVoiceHandler::new(); 8];
        voices = [crate::pivot_voice::PivotVoice::new(); 8];
    }

    connections {
        // MIDI parsing
        midi_in -> midi_parser.midi_in;

        // Route MIDI events through voice allocator
        midi_parser.note_on -> voice_allocator.note_on;
        midi_parser.note_off -> voice_allocator.note_off;

        // Voice allocator routes events to voice handlers
        voice_allocator.voices -> voice_handlers.note_on;
        voice_allocator.voices -> voice_handlers.note_off;

        // Voice handlers to voices
        voice_handlers.frequency -> voices.frequency;
        voice_handlers.gate -> voices.gate;

        // Broadcast OP3 parameters to all voices
        op3_ratio -> voices.op3_ratio;
        op3_level -> voices.op3_level;
        op3_feedback -> voices.op3_feedback;
        op3_attack -> voices.op3_attack;
        op3_decay -> voices.op3_decay;
        op3_sustain -> voices.op3_sustain;
        op3_release -> voices.op3_release;

        // Broadcast OP2 parameters to all voices
        op2_ratio -> voices.op2_ratio;
        op2_level -> voices.op2_level;
        op2_feedback -> voices.op2_feedback;
        op2_attack -> voices.op2_attack;
        op2_decay -> voices.op2_decay;
        op2_sustain -> voices.op2_sustain;
        op2_release -> voices.op2_release;

        // Broadcast OP1 parameters to all voices
        op1_ratio -> voices.op1_ratio;
        op1_attack -> voices.op1_attack;
        op1_decay -> voices.op1_decay;
        op1_sustain -> voices.op1_sustain;
        op1_release -> voices.op1_release;

        // Broadcast route parameter to all voices
        route -> voices.route;

        // Broadcast filter parameters to all voices
        cutoff -> voices.cutoff;
        resonance -> voices.resonance;
        filter_attack -> voices.filter_attack;
        filter_decay -> voices.filter_decay;
        filter_sustain -> voices.filter_sustain;
        filter_release -> voices.filter_release;
        filter_env_amount -> voices.filter_env_amount;

        // Mix voices
        voices.audio_out -> audio_out;
    }
}

struct AudioContext {
    synth: PivotGraph,
    channels: usize,
}

fn build_audio_context(sample_rate: f32, channels: usize) -> AudioContext {
    let mut synth = PivotGraph::new();
    synth.init(sample_rate);

    AudioContext { synth, channels }
}

fn audio_callback(
    data: &mut [f32],
    context: &mut AudioContext,
    param_rx: &Receiver<ParamChange>,
    midi_rx: &Receiver<midi_input::RawMidiBytes>,
) {
    use oscen::graph::{EventInstance, EventPayload};
    use oscen::midi::RawMidiMessage;

    // Handle incoming MIDI events
    while let Ok(raw_midi) = midi_rx.try_recv() {
        let msg = RawMidiMessage::new(&raw_midi.bytes);
        let event = EventInstance {
            frame_offset: 0,
            payload: EventPayload::Object(std::sync::Arc::new(msg)),
        };
        let _ = context.synth.midi_in.try_push(event);
    }

    // Handle parameter changes
    while let Ok(change) = param_rx.try_recv() {
        match change {
            ParamChange::Op3Ratio(value) => context.synth.op3_ratio = value,
            ParamChange::Op3Level(value) => context.synth.op3_level = value,
            ParamChange::Op3Feedback(value) => context.synth.op3_feedback = value,
            ParamChange::Op3Attack(value) => context.synth.op3_attack = value,
            ParamChange::Op3Decay(value) => context.synth.op3_decay = value,
            ParamChange::Op3Sustain(value) => context.synth.op3_sustain = value,
            ParamChange::Op3Release(value) => context.synth.op3_release = value,
            ParamChange::Op2Ratio(value) => context.synth.op2_ratio = value,
            ParamChange::Op2Level(value) => context.synth.op2_level = value,
            ParamChange::Op2Feedback(value) => context.synth.op2_feedback = value,
            ParamChange::Op2Attack(value) => context.synth.op2_attack = value,
            ParamChange::Op2Decay(value) => context.synth.op2_decay = value,
            ParamChange::Op2Sustain(value) => context.synth.op2_sustain = value,
            ParamChange::Op2Release(value) => context.synth.op2_release = value,
            ParamChange::Op1Attack(value) => context.synth.op1_attack = value,
            ParamChange::Op1Decay(value) => context.synth.op1_decay = value,
            ParamChange::Op1Sustain(value) => context.synth.op1_sustain = value,
            ParamChange::Op1Release(value) => context.synth.op1_release = value,
            ParamChange::Route(value) => context.synth.route = value,
            ParamChange::Cutoff(value) => context.synth.cutoff = value,
            ParamChange::Resonance(value) => context.synth.resonance = value,
            ParamChange::FilterAttack(value) => context.synth.filter_attack = value,
            ParamChange::FilterDecay(value) => context.synth.filter_decay = value,
            ParamChange::FilterSustain(value) => context.synth.filter_sustain = value,
            ParamChange::FilterRelease(value) => context.synth.filter_release = value,
            ParamChange::FilterEnvAmount(value) => context.synth.filter_env_amount = value,
        }
    }

    for frame in data.chunks_mut(context.channels) {
        context.synth.process();
        let mono = context.synth.audio_out;

        // Write to output channels
        if context.channels >= 2 {
            frame[0] = mono;
            frame[1] = mono;
            for sample in frame.iter_mut().skip(2) {
                *sample = 0.0;
            }
        } else if context.channels == 1 {
            frame[0] = mono;
        }
    }
}

fn main() -> Result<()> {
    let (param_tx, param_rx) = mpsc::channel();
    let (midi_tx, midi_rx) = mpsc::channel();
    let _midi_connection = MidiConnection::new(midi_tx.clone())?;

    thread::Builder::new()
        .spawn(move || {
            let host = cpal::default_host();
            let device = match host.default_output_device() {
                Some(device) => device,
                None => {
                    eprintln!("No output device available");
                    return;
                }
            };

            let default_config = match device.default_output_config() {
                Ok(config) => config,
                Err(err) => {
                    eprintln!("Failed to fetch default output config: {}", err);
                    return;
                }
            };

            let config = cpal::StreamConfig {
                channels: default_config.channels(),
                sample_rate: default_config.sample_rate(),
                buffer_size: cpal::BufferSize::Fixed(512),
            };

            let sample_rate = config.sample_rate.0 as f32;
            let mut audio_context = build_audio_context(sample_rate, config.channels as usize);

            let stream = match device.build_output_stream(
                &config,
                move |data: &mut [f32], _| {
                    audio_callback(data, &mut audio_context, &param_rx, &midi_rx);
                },
                |err| eprintln!("Audio stream error: {}", err),
                None,
            ) {
                Ok(stream) => stream,
                Err(err) => {
                    eprintln!("Failed to build output stream: {}", err);
                    return;
                }
            };

            if let Err(err) = stream.play() {
                eprintln!("Failed to start audio stream: {}", err);
                return;
            }

            loop {
                thread::sleep(Duration::from_millis(100));
            }
        })
        .context("failed to spawn audio thread")?;

    run_ui(param_tx)?;
    Ok(())
}

fn run_ui(tx: Sender<ParamChange>) -> Result<()> {
    let ui = SynthWindow::new()?;

    // OP3 knobs
    {
        let tx = tx.clone();
        ui.on_op3_ratio_edited(move |value| {
            let _ = tx.send(ParamChange::Op3Ratio(value));
        });
    }
    {
        let tx = tx.clone();
        ui.on_op3_level_edited(move |value| {
            let _ = tx.send(ParamChange::Op3Level(value));
        });
    }
    {
        let tx = tx.clone();
        ui.on_op3_feedback_edited(move |value| {
            let _ = tx.send(ParamChange::Op3Feedback(value));
        });
    }
    {
        let tx = tx.clone();
        ui.on_op3_attack_edited(move |value| {
            let _ = tx.send(ParamChange::Op3Attack(value));
        });
    }
    {
        let tx = tx.clone();
        ui.on_op3_decay_edited(move |value| {
            let _ = tx.send(ParamChange::Op3Decay(value));
        });
    }
    {
        let tx = tx.clone();
        ui.on_op3_sustain_edited(move |value| {
            let _ = tx.send(ParamChange::Op3Sustain(value));
        });
    }
    {
        let tx = tx.clone();
        ui.on_op3_release_edited(move |value| {
            let _ = tx.send(ParamChange::Op3Release(value));
        });
    }

    // OP2 knobs
    {
        let tx = tx.clone();
        ui.on_op2_ratio_edited(move |value| {
            let _ = tx.send(ParamChange::Op2Ratio(value));
        });
    }
    {
        let tx = tx.clone();
        ui.on_op2_level_edited(move |value| {
            let _ = tx.send(ParamChange::Op2Level(value));
        });
    }
    {
        let tx = tx.clone();
        ui.on_op2_feedback_edited(move |value| {
            let _ = tx.send(ParamChange::Op2Feedback(value));
        });
    }
    {
        let tx = tx.clone();
        ui.on_op2_attack_edited(move |value| {
            let _ = tx.send(ParamChange::Op2Attack(value));
        });
    }
    {
        let tx = tx.clone();
        ui.on_op2_decay_edited(move |value| {
            let _ = tx.send(ParamChange::Op2Decay(value));
        });
    }
    {
        let tx = tx.clone();
        ui.on_op2_sustain_edited(move |value| {
            let _ = tx.send(ParamChange::Op2Sustain(value));
        });
    }
    {
        let tx = tx.clone();
        ui.on_op2_release_edited(move |value| {
            let _ = tx.send(ParamChange::Op2Release(value));
        });
    }

    // OP1 envelope knobs
    {
        let tx = tx.clone();
        ui.on_op1_attack_edited(move |value| {
            let _ = tx.send(ParamChange::Op1Attack(value));
        });
    }
    {
        let tx = tx.clone();
        ui.on_op1_decay_edited(move |value| {
            let _ = tx.send(ParamChange::Op1Decay(value));
        });
    }
    {
        let tx = tx.clone();
        ui.on_op1_sustain_edited(move |value| {
            let _ = tx.send(ParamChange::Op1Sustain(value));
        });
    }
    {
        let tx = tx.clone();
        ui.on_op1_release_edited(move |value| {
            let _ = tx.send(ParamChange::Op1Release(value));
        });
    }

    // Route knob
    {
        let tx = tx.clone();
        ui.on_route_edited(move |value| {
            let _ = tx.send(ParamChange::Route(value));
        });
    }

    // Filter knobs
    {
        let tx = tx.clone();
        ui.on_cutoff_edited(move |value| {
            let _ = tx.send(ParamChange::Cutoff(value));
        });
    }
    {
        let tx = tx.clone();
        ui.on_resonance_edited(move |value| {
            let _ = tx.send(ParamChange::Resonance(value));
        });
    }
    {
        let tx = tx.clone();
        ui.on_filter_attack_edited(move |value| {
            let _ = tx.send(ParamChange::FilterAttack(value));
        });
    }
    {
        let tx = tx.clone();
        ui.on_filter_decay_edited(move |value| {
            let _ = tx.send(ParamChange::FilterDecay(value));
        });
    }
    {
        let tx = tx.clone();
        ui.on_filter_sustain_edited(move |value| {
            let _ = tx.send(ParamChange::FilterSustain(value));
        });
    }
    {
        let tx = tx.clone();
        ui.on_filter_release_edited(move |value| {
            let _ = tx.send(ParamChange::FilterRelease(value));
        });
    }
    {
        let tx = tx.clone();
        ui.on_filter_env_amount_edited(move |value| {
            let _ = tx.send(ParamChange::FilterEnvAmount(value));
        });
    }

    // Set default values
    ui.set_op3_ratio(3.0);
    ui.set_op3_level(0.5);
    ui.set_op3_feedback(0.0);
    ui.set_op3_attack(0.01);
    ui.set_op3_decay(0.1);
    ui.set_op3_sustain(0.7);
    ui.set_op3_release(0.3);
    ui.set_op2_ratio(2.0);
    ui.set_op2_level(0.5);
    ui.set_op2_feedback(0.0);
    ui.set_op2_attack(0.01);
    ui.set_op2_decay(0.1);
    ui.set_op2_sustain(0.7);
    ui.set_op2_release(0.3);
    ui.set_op1_attack(0.01);
    ui.set_op1_decay(0.2);
    ui.set_op1_sustain(0.8);
    ui.set_op1_release(0.5);
    ui.set_route(0.0);
    ui.set_cutoff(2000.0);
    ui.set_resonance(0.707);
    ui.set_filter_attack(0.01);
    ui.set_filter_decay(0.2);
    ui.set_filter_sustain(0.5);
    ui.set_filter_release(0.3);
    ui.set_filter_env_amount(0.0);

    ui.run().context("failed to run UI")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_midi_parser_and_allocator() {
        use oscen::graph::{EventInstance, EventPayload};
        use oscen::midi::{NoteOnEvent, RawMidiMessage};
        use oscen::prelude::*;

        // Test just the parser and allocator in isolation
        let mut parser = MidiParser::new();
        let mut allocator = VoiceAllocator::<8>::new();
        parser.init(48_000.0);
        allocator.init(48_000.0);

        // Push raw MIDI to parser
        let note_on_bytes = [0x90, 60, 100];
        let msg = RawMidiMessage::new(&note_on_bytes);
        let event = EventInstance {
            frame_offset: 0,
            payload: EventPayload::Object(std::sync::Arc::new(msg)),
        };
        let _ = parser.midi_in.try_push(event);

        // Process parser - should emit note_on event
        parser.clear_event_outputs();
        let midi_events: arrayvec::ArrayVec<_, 32> = parser.midi_in.iter().cloned().collect();
        parser.handle_midi_in_events(&midi_events);
        parser.process();

        eprintln!("Parser note_on queue len: {}", parser.note_on.len());
        for (i, ev) in parser.note_on.iter().enumerate() {
            eprintln!("  Event {}: {:?}", i, ev.payload);
        }

        // Copy parser output to allocator input
        allocator.note_on.clear();
        for ev in parser.note_on.iter() {
            let _ = allocator.note_on.try_push(ev.clone());
        }

        eprintln!("Allocator note_on queue len: {}", allocator.note_on.len());

        // Process allocator - should route to voices[0]
        allocator.clear_event_outputs();
        let note_on_events: arrayvec::ArrayVec<_, 32> = allocator.note_on.iter().cloned().collect();
        allocator.handle_note_on_events(&note_on_events);
        allocator.process();

        eprintln!("Allocator voices[0] queue len: {}", allocator.voices[0].len());
        for (i, ev) in allocator.voices[0].iter().enumerate() {
            eprintln!("  Voice 0 event {}: {:?}", i, ev.payload);
        }

        // Now test a handler
        let mut handler = MidiVoiceHandler::new();
        handler.init(48_000.0);

        // Copy allocator voice output to handler input
        handler.note_on.clear();
        for ev in allocator.voices[0].iter() {
            let _ = handler.note_on.try_push(ev.clone());
        }

        eprintln!("Handler note_on queue len: {}", handler.note_on.len());

        // Process handler
        handler.clear_event_outputs();
        let handler_events: arrayvec::ArrayVec<_, 32> = handler.note_on.iter().cloned().collect();
        handler.handle_note_on_events(&handler_events);
        handler.process();

        eprintln!("Handler frequency after processing: {}", handler.frequency);
        eprintln!("Handler gate queue len: {}", handler.gate.len());

        assert!(
            (handler.frequency - 261.63).abs() < 1.0,
            "Expected frequency ~261.63 (C4), got {}",
            handler.frequency
        );
    }

    #[test]
    fn test_midi_note_produces_sound() {
        use oscen::graph::{EventInstance, EventPayload};
        use oscen::midi::{NoteOnEvent, RawMidiMessage};

        let mut synth = PivotGraph::new();
        synth.init(48_000.0);

        // Send note on: channel 0, note 60, velocity 100
        let note_on = [0x90, 60, 100];
        let msg = RawMidiMessage::new(&note_on);
        let event = EventInstance {
            frame_offset: 0,
            payload: EventPayload::Object(std::sync::Arc::new(msg)),
        };
        let push_result = synth.midi_in.try_push(event);
        eprintln!("MIDI push result: {:?}", push_result);

        // Process one frame to let MIDI propagate
        synth.process();

        // Check all handlers
        eprintln!("After 1 frame:");
        for i in 0..8 {
            eprintln!("  Handler {} frequency: {}", i, synth.voice_handlers[i].frequency);
        }

        // Process more frames
        for _ in 0..9 {
            synth.process();
        }

        eprintln!("After 10 frames:");
        eprintln!("  Handler 0 frequency: {}", synth.voice_handlers[0].frequency);
        eprintln!("  Voice 0 frequency: {}", synth.voices[0].frequency);

        // Also try sending directly to a voice handler to test
        eprintln!("\nDirect test: sending NoteOnEvent directly to handler[0]");
        let direct_event = EventInstance {
            frame_offset: 0,
            payload: EventPayload::Object(std::sync::Arc::new(NoteOnEvent {
                note: 60,
                velocity: 0.8,
            })),
        };
        let _ = synth.voice_handlers[0].note_on.try_push(direct_event);
        synth.process();
        eprintln!("  Handler 0 frequency after direct push: {}", synth.voice_handlers[0].frequency);
        eprintln!("  Voice 0 frequency after direct push: {}", synth.voices[0].frequency);

        let mut max_output = 0.0f32;
        for i in 0..8192 {
            synth.process();
            let sample = synth.audio_out.abs();
            if sample > max_output {
                max_output = sample;
                if i < 100 || sample > 0.001 {
                    eprintln!("Frame {}: output={:.6}", i, sample);
                }
            }
        }

        // Debug: check voice state
        eprintln!("Final state:");
        eprintln!("  Voice 0 frequency: {}", synth.voices[0].frequency);
        eprintln!("  Handler 0 frequency: {}", synth.voice_handlers[0].frequency);

        assert!(max_output > 0.0001, "Expected sound output, got max={}", max_output);
    }

    #[test]
    fn test_voice_directly() {
        use oscen::graph::{EventInstance, EventPayload};

        // Test PivotVoice directly without the full graph
        let mut voice = crate::pivot_voice::PivotVoice::new();
        voice.init(48_000.0);

        // Set frequency
        voice.frequency = 261.63; // C4

        // Send gate on event
        let gate_on = EventInstance {
            frame_offset: 0,
            payload: EventPayload::Scalar(1.0),
        };
        let _ = voice.gate.try_push(gate_on);

        let mut max_output = 0.0f32;
        for i in 0..4800 {
            voice.process();
            let sample = voice.audio_out.abs();
            if sample > max_output {
                max_output = sample;
                if i < 50 || (i % 480 == 0) {
                    eprintln!("Voice frame {}: output={:.6}", i, sample);
                }
            }
        }

        eprintln!("Voice max output: {}", max_output);
        assert!(max_output > 0.0001, "Expected voice output, got max={}", max_output);
    }
}
