mod midi_input;

use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result};
use arrayvec;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use oscen::envelope::adsr::{AdsrEnvelope, AdsrEnvelopeEndpoints};
use oscen::filters::tpt::{TptFilter, TptFilterEndpoints};
use oscen::midi::{MidiParserEndpoints, MidiVoiceHandlerEndpoints};
use oscen::oscillators::PolyBlepOscillatorEndpoints;
use oscen::{
    graph, queue_raw_midi, MidiParser, MidiVoiceHandler, PolyBlepOscillator, VoiceAllocator,
    VoiceAllocatorEndpoints,
};
use slint::ComponentHandle;

use midi_input::MidiConnection;

slint::include_modules!();

#[derive(Clone, Copy, Debug)]
enum ParamChange {
    Cutoff(f32),
    Q(f32),
    Volume(f32),
    Attack(f32),
    Decay(f32),
    Sustain(f32),
    Release(f32),
}

// Define a Voice subgraph that encapsulates osc, filter, and envelope
graph! {
    name: Voice;

    input value frequency = 440.0;
    input event gate;
    input value cutoff = 3000.0;
    input value q = 0.707;
    input value attack = 0.01;
    input value decay = 0.1;
    input value sustain = 0.7;
    input value release = 0.2;

    output stream audio;

    node {
        osc = PolyBlepOscillator::saw(440.0, 0.6);
        filter = TptFilter::new(3000.0, 0.707);
        envelope = AdsrEnvelope::new(0.01, 0.1, 0.7, 0.2);
    }

    connection {
        frequency -> osc.frequency();
        gate -> envelope.gate();
        cutoff -> filter.cutoff();
        q -> filter.q();
        attack -> envelope.attack();
        decay -> envelope.decay();
        sustain -> envelope.sustain();
        release -> envelope.release();

        osc.output() -> filter.input();
        envelope.output() -> filter.f_mod();

        filter.output() * envelope.output() -> audio;
    }
}

// Main polyphonic synth with 4 voices using VoiceAllocator
graph! {
    name: PolySynthGraph;

    //TODD: hoisted endpoints
    input value cutoff = 3000.0;
    input value q = 0.707;
    input value volume = 0.8;
    input value attack = 0.01;
    input value decay = 0.1;
    input value sustain = 0.7;
    input value release = 0.2;

    output stream audio_out;

    node {
        midi_parser = MidiParser::new();
        voice_allocator = VoiceAllocator<8>::new();
        voice_handlers = [MidiVoiceHandler::new(); 8];
        voices = [Voice::new(sample_rate); 8];
    }

    connection {
        // Connect MIDI parser to voice allocator
        midi_parser.note_on() -> voice_allocator.note_on();
        midi_parser.note_off() -> voice_allocator.note_off();

        // Broadcast voice allocator outputs to all voice handlers
        voice_allocator.voices() -> voice_handlers.note_on();
        voice_allocator.voices() -> voice_handlers.note_off();

        // Connect voice handlers to voices (array-to-array)
        voice_handlers.frequency() -> voices.frequency();
        voice_handlers.gate() -> voices.gate();

        // Broadcast shared parameters to all voices (scalar-to-array)
        cutoff -> voices.cutoff();
        q -> voices.q();
        attack -> voices.attack();
        decay -> voices.decay();
        sustain -> voices.sustain();
        release -> voices.release();

        // Mix all voices with master volume
        voices.audio() * volume -> audio_out;
    }
}

struct AudioContext {
    synth: PolySynthGraph,
    channels: usize,
    frame_count: u64,
    total_process_time_ns: u64,
}

fn build_audio_context(sample_rate: f32, channels: usize) -> AudioContext {
    AudioContext {
        synth: PolySynthGraph::new(sample_rate),
        channels,
        frame_count: 0,
        total_process_time_ns: 0,
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
            context.synth.midi_parser.midi_in(),
            0,
            &raw_midi.bytes,
        );
    }

    // Handle parameter changes
    while let Ok(change) = param_rx.try_recv() {
        match change {
            ParamChange::Cutoff(value) => {
                context
                    .synth
                    .graph
                    .set_value_with_ramp(context.synth.cutoff, value, 1323);
            }
            ParamChange::Q(value) => {
                context
                    .synth
                    .graph
                    .set_value_with_ramp(context.synth.q, value, 441);
            }
            ParamChange::Volume(value) => {
                context
                    .synth
                    .graph
                    .set_value_with_ramp(context.synth.volume, value, 441);
            }
            ParamChange::Attack(value) => {
                context.synth.graph.set_value(context.synth.attack, value);
            }
            ParamChange::Decay(value) => {
                context.synth.graph.set_value(context.synth.decay, value);
            }
            ParamChange::Sustain(value) => {
                context.synth.graph.set_value(context.synth.sustain, value);
            }
            ParamChange::Release(value) => {
                context.synth.graph.set_value(context.synth.release, value);
            }
        }
    }

    for frame in data.chunks_mut(context.channels) {
        let start = std::time::Instant::now();

        if let Err(err) = context.synth.graph.process() {
            eprintln!("Graph processing error: {}", err);
            for sample in frame.iter_mut() {
                *sample = 0.0;
            }
            continue;
        }

        let elapsed = start.elapsed();
        context.total_process_time_ns += elapsed.as_nanos() as u64;
        context.frame_count += 1;

        // Print stats every 5 seconds
        if context.frame_count % (48000 * 5) == 0 {
            let avg_ns = context.total_process_time_ns / context.frame_count;
            let avg_us = avg_ns as f64 / 1000.0;
            eprintln!(
                "Avg process time: {:.2} µs/frame ({} frames)",
                avg_us, context.frame_count
            );
        }

        let value = context
            .synth
            .graph
            .get_value(&context.synth.audio_out)
            .unwrap_or(0.0);

        // Debug: check for NaN or denormals
        if context.frame_count < 100 && (value.is_nan() || value.abs() < 1e-20 && value != 0.0) {
            eprintln!(
                "Frame {}: suspicious value = {}",
                context.frame_count, value
            );
        }

        for sample in frame.iter_mut() {
            *sample = value;
        }
    }
}

fn main() -> Result<()> {
    let (param_tx, param_rx) = mpsc::channel();
    let (midi_tx, midi_rx) = mpsc::channel();
    let _midi_connection = MidiConnection::new(midi_tx.clone())?;

    thread::spawn(move || {
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
            buffer_size: cpal::BufferSize::Fixed(2048), // Increased from 512 to give 4x more time
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
    });

    run_ui(param_tx)?;
    Ok(())
}

fn run_ui(tx: Sender<ParamChange>) -> Result<()> {
    let ui = SynthWindow::new()?;

    // Wire up cutoff frequency knob
    {
        let tx = tx.clone();
        ui.on_cutoff_frequency_edited(move |value| {
            let _ = tx.send(ParamChange::Cutoff(value));
        });
    }

    // Wire up Q factor knob
    {
        let tx = tx.clone();
        ui.on_q_factor_edited(move |value| {
            let _ = tx.send(ParamChange::Q(value));
        });
    }

    // Wire up volume knob
    {
        let tx = tx.clone();
        ui.on_volume_edited(move |value| {
            let _ = tx.send(ParamChange::Volume(value));
        });
    }

    // Wire up attack knob
    {
        let tx = tx.clone();
        ui.on_attack_edited(move |value| {
            let _ = tx.send(ParamChange::Attack(value));
        });
    }

    // Wire up decay knob
    {
        let tx = tx.clone();
        ui.on_decay_edited(move |value| {
            let _ = tx.send(ParamChange::Decay(value));
        });
    }

    // Wire up sustain knob
    {
        let tx = tx.clone();
        ui.on_sustain_edited(move |value| {
            let _ = tx.send(ParamChange::Sustain(value));
        });
    }

    // Wire up release knob
    {
        let tx = tx.clone();
        ui.on_release_edited(move |value| {
            let _ = tx.send(ParamChange::Release(value));
        });
    }

    // Set default values
    ui.set_cutoff_frequency(3000.0);
    ui.set_q_factor(0.707);
    ui.set_volume(0.8);
    ui.set_attack(0.01);
    ui.set_decay(0.1);
    ui.set_sustain(0.7);
    ui.set_release(0.2);

    ui.run().context("failed to run UI")
}
