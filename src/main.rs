mod midi;

use core::cmp::Ordering;
use core::time::Duration;
use crossbeam::crossbeam_channel::{unbounded, Receiver, Sender};
use nannou::{prelude::*, ui::prelude::*};
use nannou_audio as audio;
use nannou_audio::Buffer;
use pitch_calc::calc::hz_from_step;
use std::thread;
use swell::envelopes::{
    off, on, set_attack, set_decay, set_release, set_sustain_level, SustainSynth,
};
use swell::filters::{biquad_off, biquad_on, set_lphpf, BiquadFilter};
use swell::graph::{arc, cv, fix, Graph, Real, Set};
use swell::operators::{set_knob, Lerp, Lerp3, Mixer, Modulator};
use swell::oscillators::{set_hz, SawOsc, SineOsc, SquareOsc, TriangleOsc, WhiteNoise, MidiPitch};

use midi::listen_midi;

fn main() {
    nannou::app(model).update(update).run();
}

struct Model {
    stream: audio::Stream<Synth>,
    midi_receiver: Receiver<Vec<u8>>,
    osc1_freq: Real,
}

struct Synth {
    midi_pitch: MidiPitch,
    voice: Graph,
}

fn build_synth(midi_pitch: MidiPitch) -> Graph {
    // Oscillator 1
    let sine1 = SineOsc::wrapped(); //1
    let saw1 = SawOsc::wrapped(); //2
    let square1 = SquareOsc::wrapped(); //3
    let triangle1 = TriangleOsc::wrapped(); //4

    let modulator_osc1 = Modulator::wrapped(7, 0.0, 0.0, 0.0);

    sine1.hz = cv(modulator_osc1);

    // Sub Oscillators for Osc 1
    let sub1 = SquareOsc::wrapped(); //5
    let sub2 = SquareOsc::wrapped(); //6

    // Oscillator 2
    let sine2 = SineOsc::wrapped(); //7
    let saw2 = SawOsc::wrapped(); //8
    let square2 = SquareOsc::wrapped(); //9
    let triangle2 = TriangleOsc::wrapped(); //10

    let modulator_osc2 = Modulator::wrapped(12, 0.0, 0.0, 0.0);

    // Noise
    let noise = WhiteNoise::wrapped(); //11

    // LFO
    let tri_lfo = TriangleOsc::wrapped(); //12
    let square_lfo = SquareOsc::wrapped(); //13

    // Mixers
    // sine1 + saw1
    let mixer1 = Mixer::wrapped(vec![1, 2]); //14
    // square1 + sub1
    let mixer2 = Mixer::wrapped(vec![3, 5]); //15
    // mixer1 + mixer2
    let mixer3 = Mixer::wrapped(vec![14, 15]); //16

    // Envelope Generator
    let adsr = SustainSynth::wrapped(16); //17

    Graph::new(vec![midi_pitch,
                        sine1, saw1, square1, triangle1,
                        sub1, sub2,
                        sine2, saw2, square2, triangle2,
                        noise,
                        tri_lfo, square_lfo,
                        mixer1, mixer2, mixer3,
                        adsr])
}

fn model(app: &App) -> Model {
    let (midi_sender, midi_receiver) = unbounded();

    thread::spawn(|| match listen_midi(midi_sender) {
        Ok(_) => (),
        Err(err) => println!("Error: {}", err),
    });

    // Create a window to receive key pressed events.
    app.set_loop_mode(LoopMode::Rate {
        update_interval: Duration::from_millis(1),
    });

    let _window = app.new_window().size(900, 520).view(view).build().unwrap();

    // Create audio host
    let audio_host = audio::Host::new();

    let midi_pitch = MidiPitch::new();

    // Build synth
    let synth = Synth {
        midi_pitch,
        voice: build_synth(),
    };

    let stream = audio_host
        .new_output_stream(synth)
        .render(audio)
        .build()
        .unwrap();

    Model {
        stream,
        midi_receiver,
        osc1_freq: 0.,
    }
}

// A function that renders the given `Audio` to the given `Buffer`.
// In this case we play a simple sine wave at the audio's current frequency in `hz`.
fn audio(synth: &mut Synth, buffer: &mut Buffer) {
    let sample_rate = buffer.sample_rate() as Real;
    for frame in buffer.frames_mut() {
        let mut amp = 0.;
        amp += synth.voice.signal(sample_rate);
        for channel in frame {
            *channel = amp as f32;
        }
    }
}

fn update(_app: &App, model: &mut Model, _update: Update) {
    let midi_messages: Vec<Vec<u8>> = model.midi_receiver.try_iter().collect();
    for message in midi_messages {
        let step = message[1];
        let hz = hz_from_step(step as f32) as Real;
        model.osc1_freq = hz;
        if message.len() == 3 {
            if message[0] == 144 {
                model
                    .stream
                    .send(move |synth| {
                        &synth.midi_pitch.set_hz(hz);
                        on(&synth.voice, 9);
                    })
                    .unwrap();
            } else if message[0] == 128 {
                model
                    .stream
                    .send(move |synth| {
                        off(&synth.voice, 9);
                    })
                    .unwrap();
            }
        }
    }
}

fn view(app: &App, model: &Model, frame: Frame) {
    let draw = app.draw();
    let c = rgb(9. / 255., 9. / 255., 44. / 255.);
    draw.background().color(c);
    if frame.nth() == 0 {
        draw.to_frame(app, &frame).unwrap()
    }
}
