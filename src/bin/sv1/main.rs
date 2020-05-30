mod midi;

// use core::cmp::Ordering;
use core::time::Duration;
use crossbeam::crossbeam_channel::{unbounded, Receiver};
use midi::{listen_midi, MidiControl, MidiPitch};
use nannou::{prelude::*};
use nannou_audio as audio;
use nannou_audio::Buffer;
use pitch_calc::calc::hz_from_step;
use std::thread;
use swell::envelopes::{off, on, Adsr};
// use swell::filters::{biquad_off, biquad_on, set_lphpf, BiquadFilter};
use swell::graph::{arc, cv, fix, ArcMutex, Graph, Real,  Signal, Tag};
use swell::operators::{Mixer, Modulator, Vca};
use swell::oscillators::{SawOsc, SineOsc, SquareOsc, TriangleOsc, WhiteNoise};

fn main() {
    nannou::app(model).run();
}

struct Model {
    stream: audio::Stream<Synth>,
}

struct Synth {
    midi: ArcMutex<Midi>,
    midi_receiver: Receiver<Vec<u8>>,
    voice: Graph,
    adsr_tag: Tag,
}

#[derive(Clone)]
struct Midi {
    midi_pitch: ArcMutex<MidiPitch>,
    midi_controls: Vec<ArcMutex<MidiControl>>,
}

fn build_synth(midi_receiver: Receiver<Vec<u8>>) -> Synth {
    //  Midi
    let midi_pitch = MidiPitch::wrapped();
    let midi_volume = MidiControl::wrapped(1);

    // Envelope Generator
    let adsr = Adsr::new(0.01, 0.0, 1.0, 0.1);
    let adsr_tag = adsr.tag();


    // LFO
    let tri_lfo = TriangleOsc::wrapped();
    let square_lfo = SquareOsc::wrapped();


    // TODO: tune these lower
    // Sub Oscillators for Osc 1
    let modulator_osc2 = Modulator::wrapped(
        tri_lfo.tag(),
        cv(midi_pitch.tag()),
        fix(0.0),
        fix(0.0),
    );

    // Oscillator 2
    let sine2 = SineOsc::with_hz(cv(modulator_osc2.tag()));
    let saw2 = SawOsc::with_hz(cv(midi_pitch.tag()));
    let square2 = SquareOsc::with_hz(cv(midi_pitch.tag()));
    let triangle2 = TriangleOsc::with_hz(cv(midi_pitch.tag()));

    let modulator_osc1 = Modulator::wrapped(
        sine2.tag(),
        cv(midi_pitch.tag()),
        fix(0.0),
        fix(0.0),
    );

    // Oscillator 1
    let sine1 = SineOsc::with_hz(cv(modulator_osc1.tag()));
    let saw1 = SawOsc::with_hz(cv(midi_pitch.tag()));
    let square1 = SquareOsc::with_hz(cv(midi_pitch.tag()));
    let triangle1 = TriangleOsc::with_hz(cv(midi_pitch.tag()));

    let sub1 = SquareOsc::with_hz(cv(midi_pitch.tag()));
    let sub2 = SquareOsc::with_hz(cv(midi_pitch.tag())); 


    // Noise
    let noise = WhiteNoise::wrapped();

    // Mixers
    // sine1 + saw1
    let mixer1 = Mixer::wrapped(vec![sine1.tag(), saw1.tag()]);
    // square1 + sub1
    let mixer2 = Mixer::wrapped(vec![square1.tag(), sub1.tag()]);
    // mixer1 + mixer2
    let mut mixer3 = Mixer::new(vec![saw1.tag()]);
    mixer3.level = cv(adsr.tag());

    let vca = Vca::wrapped(mixer3.tag(), fix(0.5));
    // let vca = Vca::wrapped("vca", mixer3.tag(), cv(midi_volume.tag()));

    let graph = Graph::new(vec![
        midi_pitch.clone(),
        midi_volume.clone(),
        arc(adsr),
        arc(sine1),
        arc(saw1),
        arc(square1),
        arc(triangle1),
        arc(sub1),
        arc(sub2),
        arc(sine2),
        arc(saw2),
        arc(square2),
        arc(triangle2),
        modulator_osc1,
        modulator_osc2,
        noise,
        tri_lfo,
        square_lfo,
        mixer1,
        mixer2,
        arc(mixer3),
        vca,
    ]);

    Synth {
        midi: arc(Midi {
            midi_pitch,
            midi_controls: vec![midi_volume],
        }),
        midi_receiver,
        voice: graph,
        adsr_tag,
    }
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

    // Build synth
    let synth = build_synth(midi_receiver);

    let stream = audio_host
        .new_output_stream(synth)
        .render(audio)
        .build()
        .unwrap();

    Model { stream }
}

// A function that renders the given `Audio` to the given `Buffer`.
// In this case we play a simple sine wave at the audio's current frequency in `hz`.
fn audio(synth: &mut Synth, buffer: &mut Buffer) {
    let midi_messages: Vec<Vec<u8>> = synth.midi_receiver.try_iter().collect();
    let adsr_tag = synth.adsr_tag;
    for message in midi_messages {
        if message.len() == 3 {
            let step = message[1];
            let hz = hz_from_step(step as f32) as Real;
            if message[0] == 144 {
                &synth
                    .midi
                    .lock()
                    .unwrap()
                    .midi_pitch
                    .lock()
                    .unwrap()
                    .set_hz(hz);
                on(&synth.voice, adsr_tag);
            } else if message[0] == 128 {
                off(&synth.voice, adsr_tag);
            } else if message[0] == 176 {
                for c in &synth.midi.lock().unwrap().midi_controls {
                    let mut control = c.lock().unwrap();
                    if control.controller == message[1] {
                        control.set_value(message[2]);
                    }
                }
            }
        }
    }

    let sample_rate = buffer.sample_rate() as Real;
    for frame in buffer.frames_mut() {
        let mut amp = 0.;
        amp += synth.voice.signal(sample_rate);
        for channel in frame {
            *channel = amp as f32;
        }
    }
}

fn view(app: &App, _model: &Model, frame: Frame) {
    let draw = app.draw();
    let c = rgb(9. / 255., 9. / 255., 44. / 255.);
    draw.background().color(c);
    if frame.nth() == 0 {
        draw.to_frame(app, &frame).unwrap()
    }
}
