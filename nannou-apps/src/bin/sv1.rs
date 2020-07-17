// use core::cmp::Ordering;
use crossbeam::crossbeam_channel::{unbounded, Receiver, Sender};
use nannou::prelude::*;
use nannou_audio as audio;
use nannou_audio::Buffer;
use std::thread;
use oscen::envelopes::Adsr;
use oscen::filters::Lpf;
use oscen::midi::{listen_midi, MidiControl, MidiPitch};
use oscen::operators::{Mixer, Modulator, Vca};
use oscen::oscillators::{SawOsc, SineOsc, SquareOsc, TriangleOsc, WhiteNoise};
use oscen::signal::{ArcMutex, Builder, Gate, Rack, Real, Signal, Tag};

fn main() {
    nannou::app(model).update(update).run();
}

#[allow(dead_code)]
struct Model {
    stream: audio::Stream<Synth>,
    scope_receiver: Receiver<f32>,
    scope_data: Vec<f32>,
}

struct Synth {
    midi: Midi,
    midi_receiver1: Receiver<Vec<u8>>,
    midi_receiver2: Receiver<Vec<u8>>,
    scope_sender: Sender<f32>,
    voice: Rack,
    adsr_tag: Tag,
}

#[derive(Clone)]
struct Midi {
    midi_pitch: ArcMutex<MidiPitch>,
    midi_controls: Vec<ArcMutex<MidiControl>>,
}

fn build_synth(
    midi_receiver1: Receiver<Vec<u8>>,
    midi_receiver2: Receiver<Vec<u8>>,
    scope_sender: Sender<f32>,
) -> Synth {
    let mut rack = Rack::new(vec![]);
    let mut midi_controls: Vec<ArcMutex<MidiControl>> = vec![];

    let midi_pitch = MidiPitch::new().rack(&mut rack);

    // Envelope Generator
    let midi_control_release = MidiControl::new(37, 1, 0.05, 1.0, 10.0).rack(&mut rack);
    midi_controls.push(midi_control_release.clone());
    let midi_control_attack = MidiControl::new(38, 1, 0.05, 1.0, 10.0).rack(&mut rack);
    midi_controls.push(midi_control_attack.clone());

    let adsr = Adsr::linear()
        .release(midi_control_release.tag())
        .attack(midi_control_attack.tag())
        .decay(0.05)
        .sustain(0.8)
        .rack(&mut rack);
    let adsr_tag = adsr.tag();

    let midi_control_tri_lfo_hz = MidiControl::new(46, 0, 0.0, 100.0, 500.0).rack_pre(&mut rack);
    midi_controls.push(midi_control_tri_lfo_hz.clone());

    // LFO's
    let tri_lfo = TriangleOsc::new()
        .hz(midi_control_tri_lfo_hz.tag())
        .rack(&mut rack);
    SquareOsc::new().rack(&mut rack);

    let midi_control_mod_hz2 = MidiControl::new(44, 0, 0.0, 440.0, 1760.0).rack_pre(&mut rack);
    midi_controls.push(midi_control_mod_hz2.clone());
    let midi_control_mod_idx2 = MidiControl::new(45, 0, 0.0, 4.0, 16.0).rack_pre(&mut rack);
    midi_controls.push(midi_control_mod_idx2.clone());

    // TODO: tune these lower
    // Sub Oscillators for Osc
    let modulator_osc2 = Modulator::new(tri_lfo.tag().into())
        .base_hz(midi_pitch.tag())
        .mod_hz(midi_control_mod_hz2.tag())
        .mod_idx(midi_control_mod_idx2.tag())
        .rack(&mut rack);

    // Oscillator 2
    let sine2 = SineOsc::new().hz(modulator_osc2.tag()).rack(&mut rack);
    SawOsc::new().hz(midi_pitch.tag()).rack(&mut rack);
    SquareOsc::new().hz(midi_pitch.tag()).rack(&mut rack);
    TriangleOsc::new().hz(midi_pitch.tag()).rack(&mut rack);

    let midi_control_mod_hz1 = MidiControl::new(43, 0, 0.0, 440.0, 1760.0).rack_pre(&mut rack);
    midi_controls.push(midi_control_mod_hz1.clone());
    let midi_control_mod_idx1 = MidiControl::new(42, 0, 0.0, 4.0, 16.0).rack_pre(&mut rack);
    midi_controls.push(midi_control_mod_idx1.clone());

    let modulator_osc1 = Modulator::new(sine2.tag())
        .base_hz(midi_pitch.tag())
        .mod_hz(midi_control_mod_hz1.tag())
        .mod_idx(midi_control_mod_idx1.tag())
        .rack(&mut rack);

    // Oscillator 1
    let midi_control_pulse_width = MidiControl::new(39, 0, 0.05, 0.5, 0.95).rack_pre(&mut rack);
    midi_controls.push(midi_control_pulse_width.clone());

    let sine1 = SineOsc::new().hz(modulator_osc1.tag()).rack(&mut rack);
    let saw1 = SawOsc::new().hz(midi_pitch.tag()).rack(&mut rack);
    let square1 = SquareOsc::new()
        .hz(midi_pitch.tag())
        .duty_cycle(midi_control_pulse_width.tag())
        .rack(&mut rack);
    let triangle1 = TriangleOsc::new().hz(midi_pitch.tag()).rack(&mut rack);

    // Sub 1 & 2
    SquareOsc::new().hz(midi_pitch.tag()).rack(&mut rack);
    SquareOsc::new().hz(midi_pitch.tag()).rack(&mut rack);

    // Noise
    let noise = WhiteNoise::new().rack(&mut rack);

    // Mixers
    let mut mixer = Mixer::new(vec![
        sine1.tag(),
        square1.tag(),
        saw1.tag(),
        triangle1.tag(),
        noise.tag(),
    ]);

    let midi_control_mix1 = MidiControl::new(32, 127, 0.0, 0.5, 1.0).rack_pre(&mut rack);
    midi_controls.push(midi_control_mix1.clone());
    let midi_control_mix2 = MidiControl::new(33, 0, 0.0, 0.5, 1.0).rack_pre(&mut rack);
    midi_controls.push(midi_control_mix2.clone());
    let midi_control_mix3 = MidiControl::new(34, 0, 0.0, 0.5, 1.0).rack_pre(&mut rack);
    midi_controls.push(midi_control_mix3.clone());
    let midi_control_mix4 = MidiControl::new(35, 0, 0.0, 0.5, 1.0).rack_pre(&mut rack);
    midi_controls.push(midi_control_mix4.clone());
    let midi_control_mix5 = MidiControl::new(36, 0, 0.0, 0.5, 1.0).rack_pre(&mut rack);
    midi_controls.push(midi_control_mix5.clone());

    let mixer = mixer
        .levels(vec![
            midi_control_mix1.tag(),
            midi_control_mix2.tag(),
            midi_control_mix3.tag(),
            midi_control_mix4.tag(),
            midi_control_mix5.tag(),
        ])
        .level(adsr.tag())
        .rack(&mut rack);

    // Filter
    let midi_control_cutoff = MidiControl::new(40, 127, 10.0, 1320.0, 25000.0).rack_pre(&mut rack);
    midi_controls.push(midi_control_cutoff.clone());
    let midi_control_resonance = MidiControl::new(41, 0, 0.707, 4.0, 10.0).rack_pre(&mut rack);
    midi_controls.push(midi_control_resonance.clone());

    let low_pass_filter = Lpf::new(mixer.tag())
        .cutoff_freq(midi_control_cutoff.tag())
        .q(midi_control_resonance.tag())
        .rack(&mut rack);

    // VCA
    let midi_control_volume = MidiControl::new(47, 64, 0.0, 0.5, 1.0).rack_pre(&mut rack);
    midi_controls.push(midi_control_volume.clone());
    Vca::new(low_pass_filter.tag())
        .level(midi_control_volume.tag())
        .rack(&mut rack);

    Synth {
        midi: Midi {
            midi_pitch,
            midi_controls,
        },
        midi_receiver1,
        midi_receiver2,
        scope_sender,
        voice: rack,
        adsr_tag,
    }
}

fn model(app: &App) -> Model {
    let (midi_sender1, midi_receiver1) = unbounded();
    let (midi_sender2, midi_receiver2) = unbounded();
    let (scope_sender, scope_receiver) = unbounded();

    thread::spawn(|| match listen_midi(midi_sender1) {
        Ok(_) => (),
        Err(err) => println!("Error: {}", err),
    });

    thread::spawn(|| match listen_midi(midi_sender2) {
        Ok(_) => (),
        Err(err) => println!("Error: {}", err),
    });

    let _window = app
        .new_window()
        .size(700, 360)
        .view(view)
        .always_on_top(true)
        .build()
        .unwrap();

    // Create audio host
    let audio_host = audio::Host::new();

    // Build synth
    let synth = build_synth(midi_receiver1, midi_receiver2, scope_sender);

    let stream = audio_host
        .new_output_stream(synth)
        .render(audio)
        .build()
        .unwrap();

    Model {
        stream,
        scope_receiver,
        scope_data: vec![],
    }
}

// A function that renders the given `Audio` to the given `Buffer`.
// In this case we play a simple sine wave at the audio's current frequency in `hz`.
fn audio(synth: &mut Synth, buffer: &mut Buffer) {
    let mut midi_messages: Vec<Vec<u8>> = synth.midi_receiver1.try_iter().collect();
    midi_messages.extend(synth.midi_receiver2.try_iter());

    let adsr_tag = synth.adsr_tag;
    for message in midi_messages {
        if message.len() == 3 {
            let midi_step = message[1] as f32;
            if message[0] == 144 {
                synth.midi.midi_pitch.lock().unwrap().step(midi_step);
                Adsr::gate_on(&synth.voice, adsr_tag);
            } else if message[0] == 128 {
                Adsr::gate_off(&synth.voice, adsr_tag);
            } else if message[0] == 176 {
                for c in &synth.midi.midi_controls {
                    let mut control = c.lock().unwrap();
                    if control.controller == message[1] {
                        control.value(message[2]);
                    }
                }
            }
        }
    }

    let sample_rate = buffer.sample_rate() as Real;
    for frame in buffer.frames_mut() {
        let amp = synth.voice.signal(sample_rate) as f32;
        for channel in frame {
            *channel = amp;
        }
        synth.scope_sender.send(amp).unwrap();
    }
}

fn update(_app: &App, model: &mut Model, _update: Update) {
    let scope_data: Vec<f32> = model.scope_receiver.try_iter().collect();
    model.scope_data = scope_data;
}

fn view(app: &App, model: &Model, frame: Frame) {
    use nannou_apps::scope;
    scope(app, &model.scope_data, frame);
}
