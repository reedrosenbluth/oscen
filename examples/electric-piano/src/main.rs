mod electric_piano_voice;
mod midi_input;
mod tremolo;

use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result};
use arrayvec;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use electric_piano_voice::{ElectricPianoVoiceNode, ElectricPianoVoiceNodeEndpoints};
use oscen::midi::{MidiParserEndpoints, MidiVoiceHandlerEndpoints};
use oscen::{graph, queue_raw_midi, MidiParser, MidiVoiceHandler, VoiceAllocator, VoiceAllocatorEndpoints};
use slint::ComponentHandle;
use tremolo::{Tremolo, TremoloEndpoints};

use midi_input::MidiConnection;

slint::include_modules!();

#[derive(Clone, Copy, Debug)]
enum ParamChange {
    Brightness(f32),
    VelocityScaling(f32),
    DecayRate(f32),
    HarmonicDecay(f32),
    KeyScaling(f32),
    ReleaseRate(f32),
    VibratoIntensity(f32),
    VibratoSpeed(f32),
}

// Electric piano voice using combined oscillator+envelope node (32 harmonics)
graph! {
    name: ElectricPianoVoice;

    input value frequency = 440.0;
    input event gate;
    input value brightness = 0.5;
    input value velocity_scaling = 0.5;
    input value decay_rate = 0.5;
    input value harmonic_decay = 0.5;
    input value key_scaling = 0.5;
    input value release_rate = 0.5;

    output stream audio;

    node {
        // Combined node that generates 32 harmonics with per-harmonic envelopes
        // This matches the CMajor architecture where envelope amplitudes are applied
        // to each oscillator individually before summing
        voice = ElectricPianoVoiceNode::new(sample_rate);
    }

    connection {
        // Connect all inputs to the combined voice node
        frequency -> voice.frequency;
        gate -> voice.gate;
        brightness -> voice.brightness;
        velocity_scaling -> voice.velocity_scaling;
        decay_rate -> voice.decay_rate;
        harmonic_decay -> voice.harmonic_decay;
        key_scaling -> voice.key_scaling;
        release_rate -> voice.release_rate;

        // Output from voice (harmonics with per-harmonic envelopes applied)
        voice.output -> audio;
    }
}

// Main polyphonic electric piano with 16 voices and tremolo
graph! {
    name: ElectricPianoGraph;

    input value brightness = 0.5;
    input value velocity_scaling = 0.5;
    input value decay_rate = 0.5;
    input value harmonic_decay = 0.5;
    input value key_scaling = 0.5;
    input value release_rate = 0.5;
    input value vibrato_intensity = 0.3;
    input value vibrato_speed = 5.0;

    output stream left_out;
    output stream right_out;

    node {
        midi_parser = MidiParser::new();
        voice_allocator = VoiceAllocator<16>::new();
        voice_handlers = [MidiVoiceHandler::new(); 16];
        voices = [ElectricPianoVoice::new(sample_rate); 16];
        tremolo = Tremolo::new(sample_rate);
    }

    connection {
        // MIDI routing
        midi_parser.note_on -> voice_allocator.note_on;
        midi_parser.note_off -> voice_allocator.note_off;

        // Voice allocation
        voice_allocator.voices() -> voice_handlers.note_on;
        voice_allocator.voices() -> voice_handlers.note_off;

        // Voice handlers to voices
        voice_handlers.frequency -> voices.frequency;
        voice_handlers.gate -> voices.gate;

        // Broadcast parameters to all voices
        brightness -> voices.brightness;
        velocity_scaling -> voices.velocity_scaling;
        decay_rate -> voices.decay_rate;
        harmonic_decay -> voices.harmonic_decay;
        key_scaling -> voices.key_scaling;
        release_rate -> voices.release_rate;

        // Mix voices and process through tremolo
        voices.audio -> tremolo.input;
        vibrato_intensity -> tremolo.depth;
        vibrato_speed -> tremolo.rate;

        // Stereo outputs
        tremolo.left_output -> left_out;
        tremolo.right_output -> right_out;
    }
}

struct AudioContext {
    synth: ElectricPianoGraph,
    channels: usize,
}

fn build_audio_context(sample_rate: f32, channels: usize) -> AudioContext {
    AudioContext {
        synth: ElectricPianoGraph::new(sample_rate),
        channels,
    }
}

fn audio_callback(
    data: &mut [f32],
    context: &mut AudioContext,
    param_rx: &Receiver<ParamChange>,
    midi_rx: &Receiver<midi_input::RawMidiBytes>,
) {
    // Handle incoming MIDI events
    while let Ok(raw_midi) = midi_rx.try_recv() {
        queue_raw_midi(
            &mut context.synth.graph,
            context.synth.midi_parser.midi_in,
            0,
            &raw_midi.bytes,
        );
    }

    // Handle parameter changes
    while let Ok(change) = param_rx.try_recv() {
        match change {
            ParamChange::Brightness(value) => {
                context
                    .synth
                    .graph
                    .set_value_with_ramp(context.synth.brightness, value, 441);
            }
            ParamChange::VelocityScaling(value) => {
                context
                    .synth
                    .graph
                    .set_value_with_ramp(context.synth.velocity_scaling, value, 441);
            }
            ParamChange::DecayRate(value) => {
                context
                    .synth
                    .graph
                    .set_value_with_ramp(context.synth.decay_rate, value, 441);
            }
            ParamChange::HarmonicDecay(value) => {
                context
                    .synth
                    .graph
                    .set_value_with_ramp(context.synth.harmonic_decay, value, 441);
            }
            ParamChange::KeyScaling(value) => {
                context
                    .synth
                    .graph
                    .set_value_with_ramp(context.synth.key_scaling, value, 441);
            }
            ParamChange::ReleaseRate(value) => {
                context
                    .synth
                    .graph
                    .set_value_with_ramp(context.synth.release_rate, value, 441);
            }
            ParamChange::VibratoIntensity(value) => {
                context
                    .synth
                    .graph
                    .set_value_with_ramp(context.synth.vibrato_intensity, value, 441);
            }
            ParamChange::VibratoSpeed(value) => {
                context
                    .synth
                    .graph
                    .set_value_with_ramp(context.synth.vibrato_speed, value, 441);
            }
        }
    }

    for frame in data.chunks_mut(context.channels) {
        if let Err(err) = context.synth.graph.process() {
            eprintln!("Graph processing error: {}", err);
            for sample in frame.iter_mut() {
                *sample = 0.0;
            }
            continue;
        }

        // Get mono output and create stereo in the callback
        // (The tremolo's right_output field isn't being copied by the graph)
        let mono = context
            .synth
            .graph
            .get_value(&context.synth.left_out)
            .unwrap_or(0.0);

        // Write to output channels - duplicate mono to stereo
        if context.channels >= 2 {
            frame[0] = mono;
            frame[1] = mono;
            // Zero out any additional channels
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

    // Spawn audio thread with larger stack size (8MB instead of default 2MB)
    // This is needed because we have 16 voices with large harmonic arrays
    thread::Builder::new()
        .stack_size(8 * 1024 * 1024)
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

    // Wire up brightness knob
    {
        let tx = tx.clone();
        ui.on_brightness_edited(move |value| {
            let _ = tx.send(ParamChange::Brightness(value));
        });
    }

    // Wire up velocity scaling knob
    {
        let tx = tx.clone();
        ui.on_velocity_scaling_edited(move |value| {
            let _ = tx.send(ParamChange::VelocityScaling(value));
        });
    }

    // Wire up decay rate knob
    {
        let tx = tx.clone();
        ui.on_decay_rate_edited(move |value| {
            let _ = tx.send(ParamChange::DecayRate(value));
        });
    }

    // Wire up harmonic decay knob
    {
        let tx = tx.clone();
        ui.on_harmonic_decay_edited(move |value| {
            let _ = tx.send(ParamChange::HarmonicDecay(value));
        });
    }

    // Wire up key scaling knob
    {
        let tx = tx.clone();
        ui.on_key_scaling_edited(move |value| {
            let _ = tx.send(ParamChange::KeyScaling(value));
        });
    }

    // Wire up release rate knob
    {
        let tx = tx.clone();
        ui.on_release_rate_edited(move |value| {
            let _ = tx.send(ParamChange::ReleaseRate(value));
        });
    }

    // Wire up vibrato intensity knob
    {
        let tx = tx.clone();
        ui.on_vibrato_intensity_edited(move |value| {
            let _ = tx.send(ParamChange::VibratoIntensity(value));
        });
    }

    // Wire up vibrato speed knob
    {
        let tx = tx.clone();
        ui.on_vibrato_speed_edited(move |value| {
            let _ = tx.send(ParamChange::VibratoSpeed(value));
        });
    }

    // Set default values
    ui.set_brightness(0.5);
    ui.set_velocity_scaling(0.5);
    ui.set_decay_rate(0.5);
    ui.set_harmonic_decay(0.5);
    ui.set_key_scaling(0.5);
    ui.set_release_rate(0.5);
    ui.set_vibrato_intensity(0.3);
    ui.set_vibrato_speed(5.0);

    ui.run().context("failed to run UI")
}
