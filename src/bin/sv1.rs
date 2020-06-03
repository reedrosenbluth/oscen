// use core::cmp::Ordering;
use core::time::Duration;
use crossbeam::crossbeam_channel::{unbounded, Receiver, Sender};
use nannou::{prelude::*};
use nannou_audio as audio;
use nannou_audio::Buffer;
use pitch_calc::calc::hz_from_step;
use std::thread;
use swell::envelopes::{off, on, Adsr};
use swell::graph::{arc, ArcMutex, Graph, Real,  Signal, Tag};
use swell::operators::{Mixer, Modulator, Vca};
use swell::oscillators::{SawOsc, SineOsc, SquareOsc, TriangleOsc, WhiteNoise};
use swell::midi::{listen_midi, MidiControl, MidiPitch};

fn main() {
    nannou::app(model).update(update).run();
}

struct Model {
    stream: audio::Stream<Synth>,
    scope_receiver: Receiver<f32>,
    scope_data: Vec<f32>,
}

struct Synth {
    midi: ArcMutex<Midi>,
    midi_receiver1: Receiver<Vec<u8>>,
    midi_receiver2: Receiver<Vec<u8>>,
    scope_sender: Sender<f32>,
    voice: Graph,
    adsr_tag: Tag,
}

#[derive(Clone)]
struct Midi {
    midi_pitch: ArcMutex<MidiPitch>,
    midi_controls: Vec<ArcMutex<MidiControl>>,
}

fn build_synth(midi_receiver1: Receiver<Vec<u8>>, midi_receiver2: Receiver<Vec<u8>>, scope_sender: Sender<f32>) -> Synth {
    //  Midi
    let midi_pitch = MidiPitch::wrapped();
    let midi_control1 = MidiControl::wrapped(32);
    let midi_control2 = MidiControl::wrapped(33);
    let midi_control3 = MidiControl::wrapped(34);
    let midi_control4 = MidiControl::wrapped(35);
    let midi_control5 = MidiControl::wrapped(36);

    let mut midi_control6 = MidiControl::new(37);
    midi_control6.scale = 10.;
    let midi_control6 = arc(midi_control6);

    // Envelope Generator
    let mut adsr = Adsr::new(0.01, 0.0, 1.0, 0.1);
    adsr.release = midi_control6.tag().into();
    let adsr_tag = adsr.tag();

    // LFO
    let tri_lfo = TriangleOsc::wrapped();
    let square_lfo = SquareOsc::wrapped();

    // TODO: tune these lower
    // Sub Oscillators for Osc 1
    let modulator_osc2 = Modulator::wrapped(
        tri_lfo.tag(),
        midi_pitch.tag().into(),
        (0.0).into(),
        (0.0).into(),
    );

    // Oscillator 2
    let sine2 = SineOsc::with_hz(modulator_osc2.tag().into());
    let saw2 = SawOsc::with_hz(midi_pitch.tag().into());
    let square2 = SquareOsc::with_hz(midi_pitch.tag().into());
    let triangle2 = TriangleOsc::with_hz(midi_pitch.tag().into());

    let modulator_osc1 = Modulator::wrapped(
        sine2.tag(),
        midi_pitch.tag().into(),
        (0.0).into(),
        (0.0).into(),
    );

    // Oscillator 1
    let sine1 = SineOsc::with_hz(modulator_osc1.tag().into());
    let saw1 = SawOsc::with_hz(midi_pitch.tag().into());
    let square1 = SquareOsc::with_hz(midi_pitch.tag().into());
    let triangle1 = TriangleOsc::with_hz(midi_pitch.tag().into());

    let sub1 = SquareOsc::with_hz(midi_pitch.tag().into());
    let sub2 = SquareOsc::with_hz(midi_pitch.tag().into()); 


    // Noise
    let noise = WhiteNoise::wrapped();

    // Mixers
    let mut mixer = Mixer::new(vec![
        sine1.tag(),
        square1.tag(),
        saw1.tag(),
        triangle1.tag(),
        noise.tag(),
        ]);

    mixer.levels = vec![
        midi_control1.tag().into(),
        midi_control2.tag().into(),
        midi_control3.tag().into(),
        midi_control4.tag().into(),
        midi_control5.tag().into(),
        ];
    mixer.level = adsr.tag().into();

    let vca = Vca::wrapped(mixer.tag(), (0.5).into());

    let graph = Graph::new(vec![
        midi_pitch.clone(),
        midi_control1.clone(),
        midi_control2.clone(),
        midi_control3.clone(),
        midi_control4.clone(),
        midi_control5.clone(),
        midi_control6.clone(),
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
        arc(mixer),
        vca,
    ]);

    Synth {
        midi: arc(Midi {
            midi_pitch,
            midi_controls: vec![
                midi_control1,
                midi_control2,
                midi_control3,
                midi_control4,
                midi_control5,
                midi_control6,
                ],
        }),
        midi_receiver1,
        midi_receiver2,
        scope_sender,
        voice: graph,
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

    // Create a window to receive key pressed events.
    app.set_loop_mode(LoopMode::Rate {
        update_interval: Duration::from_millis(1),
    });

    let _window = app.new_window().size(900, 520).view(view).build().unwrap();

    // Create audio host
    let audio_host = audio::Host::new();

    // Build synth
    let synth = build_synth(midi_receiver1, midi_receiver2, scope_sender);

    let stream = audio_host
        .new_output_stream(synth)
        .render(audio)
        .build()
        .unwrap();

    Model { stream, scope_receiver, scope_data: vec![] }
}

// A function that renders the given `Audio` to the given `Buffer`.
// In this case we play a simple sine wave at the audio's current frequency in `hz`.
fn audio(synth: &mut Synth, buffer: &mut Buffer) {
    let mut midi_messages: Vec<Vec<u8>> = synth.midi_receiver1.try_iter().collect();
    midi_messages.extend(synth.midi_receiver2.try_iter());

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
        synth.scope_sender.send(amp as f32).unwrap();
    }
}

fn update(_app: &App, model: &mut Model, _update: Update) {
    let scope_data: Vec<f32> = model.scope_receiver.try_iter().collect();
    model.scope_data = scope_data;
}

fn view(app: &App, model: &Model, frame: Frame) {
    // Draw BG
    let draw = app.draw();
    let bg_color = rgb(9. / 255., 9. / 255., 44. / 255.);
    draw.background().color(bg_color);
    if frame.nth() == 0 {
        draw.to_frame(app, &frame).unwrap()
    }

    // Draw Oscilloscope
    let mut scope_data = model.scope_data.iter().peekable();
    let mut shifted_scope_data: Vec<f32> = vec![];

    for (i, amp) in scope_data.clone().enumerate() {
        if amp.abs() < 0.01 && scope_data.peek().unwrap_or(&amp) > &amp {
            shifted_scope_data = model.scope_data[i..].to_vec();
            break;
        }
    }

    if shifted_scope_data.len() >= 600  {
        let shifted_scope_data = shifted_scope_data[0..600].iter();
        let scope_points = shifted_scope_data.zip((0..600).into_iter()).map(|(y, x)| pt2(x as f32, y * 120.));
        
        draw.path()
            .stroke()
            .weight(2.)
            .points(scope_points)
            .color(CORNFLOWERBLUE)
            .x_y(-200., 0.);

        draw.to_frame(app, &frame).unwrap();
    }
}
