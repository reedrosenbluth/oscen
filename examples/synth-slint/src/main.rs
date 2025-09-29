use std::cell::RefCell;
use std::rc::Rc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use coremidi::{Client, InputPort, Source, Sources};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use oscen::envelope::AdsrEnvelope;
use oscen::graph::types::EventPayload;
use oscen::{Graph, InputEndpoint, OutputEndpoint, PolyBlepOscillator, TptFilter, Value, ValueKey};
use slint::ComponentHandle;

slint::include_modules!();

#[derive(Clone, Copy, Debug)]
struct SynthParams {
    cutoff_frequency: f32,
    q_factor: f32,
    volume: f32,
    attack: f32,
    decay: f32,
    sustain: f32,
    release: f32,
}

impl Default for SynthParams {
    fn default() -> Self {
        Self {
            cutoff_frequency: 3_000.0,
            q_factor: 0.707,
            volume: 0.8,
            attack: 0.01,
            decay: 0.1,
            sustain: 0.7,
            release: 0.2,
        }
    }
}

#[derive(Debug)]
enum MidiMessage {
    NoteOn { note: u8, velocity: u8 },
    NoteOff { note: u8 },
}

struct MidiConnection {
    _client: Client,
    _port: InputPort,
    _sources: Vec<Source>,
}

impl MidiConnection {
    fn new(tx: Sender<MidiMessage>) -> Result<Self> {
        let client = Client::new("oscen-midi-client")
            .map_err(|status| anyhow!("failed to create MIDI client: {status}"))?;

        let port = client
            .input_port("oscen-midi-input", move |packet_list| {
                for packet in packet_list.iter() {
                    let data = packet.data();
                    if data.len() < 3 {
                        continue;
                    }

                    let status = data[0] & 0xF0;
                    let note = data[1];
                    let velocity = data[2];

                    let message = match status {
                        0x80 => Some(MidiMessage::NoteOff { note }),
                        0x90 => {
                            if velocity == 0 {
                                Some(MidiMessage::NoteOff { note })
                            } else {
                                Some(MidiMessage::NoteOn { note, velocity })
                            }
                        }
                        _ => None,
                    };

                    if let Some(msg) = message {
                        let _ = tx.send(msg);
                    }
                }
            })
            .map_err(|status| anyhow!("failed to create MIDI input port: {status}"))?;

        if Sources::count() == 0 {
            println!("No MIDI sources detected. Connect a device to trigger notes.");
        }

        let mut sources = Vec::new();
        for source in Sources {
            if let Some(name) = source.display_name() {
                println!("Connecting to MIDI source: {}", name);
            }
            port.connect_source(&source)
                .map_err(|status| anyhow!("failed to connect MIDI source: {status}"))?;
            sources.push(source);
        }

        Ok(Self {
            _client: client,
            _port: port,
            _sources: sources,
        })
    }
}

struct AudioContext {
    graph: Graph,
    osc_freq_input: ValueKey,
    cutoff_freq_input: ValueKey,
    q_input: ValueKey,
    volume_input: ValueKey,
    attack_input: ValueKey,
    decay_input: ValueKey,
    sustain_input: ValueKey,
    release_input: ValueKey,
    gate_input: InputEndpoint,
    output: OutputEndpoint,
    channels: usize,
    current_note: Option<u8>,
}

fn build_audio_context(sample_rate: f32, channels: usize) -> AudioContext {
    let mut graph = Graph::new(sample_rate);

    let osc = graph.add_node(PolyBlepOscillator::saw(440.0, 0.6));
    let filter = graph.add_node(TptFilter::new(3_000.0, 0.707));
    let envelope = graph.add_node(AdsrEnvelope::new(0.01, 0.1, 0.7, 0.2));
    let gain = graph.add_node(Value::new(0.8));

    graph.connect(osc.output(), filter.input());
    graph.connect(envelope.output(), filter.f_mod());

    let enveloped = graph.multiply(filter.output(), envelope.output());
    let output = graph.multiply(enveloped, gain.output());

    //TODO: can this code be taken care of by graph.add_node?
    let osc_freq_input = graph
        .insert_value_input(osc.frequency(), 440.0)
        .expect("oscillator frequency endpoint");
    let cutoff_freq_input = graph
        .insert_value_input(filter.cutoff(), 3_000.0)
        .expect("filter cutoff endpoint");
    let q_input = graph
        .insert_value_input(filter.q(), 0.707)
        .expect("filter Q endpoint");
    let volume_input = graph
        .insert_value_input(gain.input(), 0.8)
        .expect("gain endpoint");

    let attack_input = graph
        .insert_value_input(envelope.attack(), 0.01)
        .expect("attack endpoint");
    let decay_input = graph
        .insert_value_input(envelope.decay(), 0.1)
        .expect("decay endpoint");
    let sustain_input = graph
        .insert_value_input(envelope.sustain(), 0.7)
        .expect("sustain endpoint");
    let release_input = graph
        .insert_value_input(envelope.release(), 0.2)
        .expect("release endpoint");

    AudioContext {
        graph,
        osc_freq_input,
        cutoff_freq_input,
        q_input,
        volume_input,
        attack_input,
        decay_input,
        sustain_input,
        release_input,
        gate_input: envelope.gate(),
        output,
        channels,
        current_note: None,
    }
}

fn midi_note_to_freq(note: u8) -> f32 {
    let semitone_offset = note as f32 - 69.0;
    440.0 * 2f32.powf(semitone_offset / 12.0)
}

fn handle_midi_message(context: &mut AudioContext, message: MidiMessage) {
    match message {
        MidiMessage::NoteOn { note, velocity } => {
            let velocity = (velocity as f32 / 127.0).clamp(0.0, 1.0);
            let freq = midi_note_to_freq(note);
            context.graph.set_value(context.osc_freq_input, freq);
            let _ =
                context
                    .graph
                    .queue_event(context.gate_input, 0, EventPayload::scalar(velocity));
            context.current_note = Some(note);
        }
        MidiMessage::NoteOff { note } => {
            if context.current_note == Some(note) {
                let _ = context
                    .graph
                    .queue_event(context.gate_input, 0, EventPayload::scalar(0.0));
                context.current_note = None;
            }
        }
    }
}

fn audio_callback(
    data: &mut [f32],
    context: &mut AudioContext,
    param_rx: &Receiver<SynthParams>,
    midi_rx: &Receiver<MidiMessage>,
) {
    while let Ok(message) = midi_rx.try_recv() {
        handle_midi_message(context, message);
    }

    let mut latest_params = None;
    while let Ok(params) = param_rx.try_recv() {
        latest_params = Some(params);
    }

    if let Some(params) = latest_params {
        let updates = [
            (context.cutoff_freq_input, params.cutoff_frequency, 1323),
            (context.q_input, params.q_factor, 441),
            (context.volume_input, params.volume, 441),
            (context.attack_input, params.attack, 0),
            (context.decay_input, params.decay, 0),
            (context.sustain_input, params.sustain, 0),
            (context.release_input, params.release, 0),
        ];

        for (key, value, ramp) in updates {
            if ramp == 0 {
                context.graph.set_value(key, value);
            } else {
                context.graph.set_value_with_ramp(key, value, ramp);
            }
        }
    }

    for frame in data.chunks_mut(context.channels) {
        if let Err(err) = context.graph.process() {
            eprintln!("Graph processing error: {}", err);
            for sample in frame.iter_mut() {
                *sample = 0.0;
            }
            continue;
        }

        let value = context.graph.get_value(&context.output).unwrap_or(0.0);
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
    });

    run_ui(param_tx)?;
    Ok(())
}

fn run_ui(tx: Sender<SynthParams>) -> Result<()> {
    let ui = SynthWindow::new()?;
    let params_state = Rc::new(RefCell::new(SynthParams::default()));

    macro_rules! wire_knob {
        ($setter:expr, $register:expr) => {{
            let params = params_state.clone();
            let tx = tx.clone();
            $register(&ui, move |value| {
                let mut state = params.borrow_mut();
                $setter(&mut state, value);
                let _ = tx.send(*state);
            });
        }};
    }

    wire_knob!(
        |state: &mut SynthParams, value| state.cutoff_frequency = value,
        SynthWindow::on_cutoff_frequency_edited
    );
    wire_knob!(
        |state: &mut SynthParams, value| state.q_factor = value,
        SynthWindow::on_q_factor_edited
    );
    wire_knob!(
        |state: &mut SynthParams, value| state.volume = value,
        SynthWindow::on_volume_edited
    );
    wire_knob!(
        |state: &mut SynthParams, value| state.attack = value,
        SynthWindow::on_attack_edited
    );
    wire_knob!(
        |state: &mut SynthParams, value| state.decay = value,
        SynthWindow::on_decay_edited
    );
    wire_knob!(
        |state: &mut SynthParams, value| state.sustain = value,
        SynthWindow::on_sustain_edited
    );
    wire_knob!(
        |state: &mut SynthParams, value| state.release = value,
        SynthWindow::on_release_edited
    );

    let defaults = SynthParams::default();
    ui.set_cutoff_frequency(defaults.cutoff_frequency);
    ui.set_q_factor(defaults.q_factor);
    ui.set_volume(defaults.volume);
    ui.set_attack(defaults.attack);
    ui.set_decay(defaults.decay);
    ui.set_sustain(defaults.sustain);
    ui.set_release(defaults.release);

    ui.run().context("failed to run UI")
}
