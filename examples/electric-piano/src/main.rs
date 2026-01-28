mod electric_piano_voice;
mod midi_input;
mod tremolo;

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
    Brightness(f32),
    VelocityScaling(f32),
    DecayRate(f32),
    HarmonicDecay(f32),
    KeyScaling(f32),
    ReleaseRate(f32),
    VibratoIntensity(f32),
    VibratoSpeed(f32),
}

// Main polyphonic electric piano with 16 voices and tremolo
graph! {
    name: ElectricPianoGraph;

    // MIDI input (raw MIDI bytes)
    input midi_in: event;

    input brightness: value = 30.0;
    input velocity_scaling: value = 50.0;
    input decay_rate: value = 90.0;
    input harmonic_decay: value = 70.0;
    input key_scaling: value = 50.0;
    input release_rate: value = 40.0;
    input vibrato_intensity: value = 0.3;
    input vibrato_speed: value = 5.0;

    output note_on_out: event;
    output note_off_out: event;

    output left_out: stream;
    output right_out: stream;

    nodes {
        midi_parser = MidiParser::new();
        voice_allocator = VoiceAllocator::<16>::new();
        voice_handlers = [MidiVoiceHandler::new(); 16];
        voices = [crate::electric_piano_voice::ElectricPianoVoiceNode::new(); 16];
        tremolo = crate::tremolo::Tremolo::new();
    }

    connections {
        // MIDI parsing
        midi_in -> midi_parser.midi_in;

        // Connect parser outputs to graph outputs
        midi_parser.note_on -> note_on_out;
        midi_parser.note_off -> note_off_out;

        // Route MIDI events through voice allocator
        midi_parser.note_on -> voice_allocator.note_on;
        midi_parser.note_off -> voice_allocator.note_off;

        // Voice allocator routes events to voice handlers via array event routing
        voice_allocator.voices -> voice_handlers.note_on;
        voice_allocator.voices -> voice_handlers.note_off;

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
        voices.output -> tremolo.input;
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
    let mut synth = ElectricPianoGraph::new();
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

    // Handle incoming MIDI events - pass raw MIDI to parser
    while let Ok(raw_midi) = midi_rx.try_recv() {
        let msg = RawMidiMessage::new(&raw_midi.bytes);
        let event = EventInstance {
            frame_offset: 0,
            payload: EventPayload::Object(std::sync::Arc::new(msg)),
        };
        let _ = context.synth.midi_in.try_push(event);
    }

    // Handle parameter changes (static graph: direct field assignment)
    while let Ok(change) = param_rx.try_recv() {
        match change {
            ParamChange::Brightness(value) => {
                context.synth.brightness = value;
            }
            ParamChange::VelocityScaling(value) => {
                context.synth.velocity_scaling = value;
            }
            ParamChange::DecayRate(value) => {
                context.synth.decay_rate = value;
            }
            ParamChange::HarmonicDecay(value) => {
                context.synth.harmonic_decay = value;
            }
            ParamChange::KeyScaling(value) => {
                context.synth.key_scaling = value;
            }
            ParamChange::ReleaseRate(value) => {
                context.synth.release_rate = value;
            }
            ParamChange::VibratoIntensity(value) => {
                context.synth.vibrato_intensity = value;
            }
            ParamChange::VibratoSpeed(value) => {
                context.synth.vibrato_speed = value;
            }
        }
    }

    for frame in data.chunks_mut(context.channels) {
        context.synth.process();

        // Static graph: direct field access for outputs
        let mono = context.synth.left_out;

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

    // Set default values (CMajor defaults, inverted for intuitive control)
    ui.set_brightness(30.0);
    ui.set_velocity_scaling(50.0);
    ui.set_decay_rate(90.0);
    ui.set_harmonic_decay(70.0);
    ui.set_key_scaling(50.0);
    ui.set_release_rate(40.0);
    ui.set_vibrato_intensity(0.3);
    ui.set_vibrato_speed(5.0);

    ui.run().context("failed to run UI")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn responds_to_midi_note_on() {
        let stats = std::thread::Builder::new()
            .stack_size(8 * 1024 * 1024)
            .spawn(|| {
                use oscen::graph::{EventInstance, EventPayload};
                use oscen::midi::RawMidiMessage;

                let mut synth = ElectricPianoGraph::new();
                synth.init(48_000.0);
                let note_on = [0x90, 60, 100];

                // For static graphs, push events directly to the input queue
                let msg = RawMidiMessage::new(&note_on);
                let event = EventInstance {
                    frame_offset: 0,
                    payload: EventPayload::Object(std::sync::Arc::new(msg)),
                };
                let _ = synth.midi_in.try_push(event);

                let mut max = 0.0;
                for i in 0..8192 {
                    synth.process();
                    let sample = synth.left_out.abs();
                    if sample > max {
                        max = sample;
                        eprintln!("New max at frame {}: {}", i, max);
                    }
                }

                // For static graphs, values are accessed directly as fields
                let voice0 = synth.voices[0].output.abs();
                let handler_freq = synth.voice_handlers[0].frequency;
                let voice_freq = synth.voices[0].frequency;


                (max, voice0, handler_freq, voice_freq)
            })
            .expect("spawn test thread")
            .join()
            .expect("thread panicked");

        assert!(
            stats.0 > 1e-4,
            "expected non-zero output after MIDI note on (voice_sample={}, handler_freq={}, voice_freq={})",
            stats.1,
            stats.2,
            stats.3,
        );
    }
}
