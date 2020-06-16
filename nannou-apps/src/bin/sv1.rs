// use core::cmp::Ordering;
use core::time::Duration;
use crossbeam::crossbeam_channel::{unbounded, Receiver, Sender};
use nannou::prelude::*;
use nannou_audio as audio;
use nannou_audio::Buffer;
use std::thread;
use swell::envelopes::{off, on, Adsr};
use swell::filters::Lpf;
use swell::midi::{listen_midi, set_step, MidiControl, MidiPitch};
use swell::operators::{Mixer, Modulator, Vca};
use swell::oscillators::{SawOsc, SineOsc, SquareOsc, TriangleOsc, WhiteNoise};
use swell::signal::{arc, ArcMutex, Builder, Rack, Real, Signal, Tag};

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
    let midi_pitch = MidiPitch::new().wrap();

    // Envelope Generator
    let midi_control_release = MidiControl::new(37, 1, 0.05, 1.0, 10.0).wrap();

    let adsr = Adsr::new()
        .release(midi_control_release.tag().into())
        .wrap();
    let adsr_tag = adsr.tag();
    
    let midi_control_tri_lfo_hz = MidiControl::new(46, 0, 0.0, 100.0, 500.0).wrap();

    // LFO
    let tri_lfo = TriangleOsc::new().hz(midi_control_tri_lfo_hz.tag().into()).wrap();
    let square_lfo = SquareOsc::new().wrap();

    let midi_control_mod_hz2 = MidiControl::new(44, 0, 0.0, 440.0, 1760.0).wrap();
    let midi_control_mod_idx2 = MidiControl::new(45, 0, 0.0, 4.0, 16.0).wrap();

    // TODO: tune these lower
    // Sub Oscillators for Osc
    let modulator_osc2 = Modulator::new(tri_lfo.tag().into())
        .base_hz(midi_pitch.tag().into())
        .mod_hz(midi_control_mod_hz2.tag().into())
        .mod_idx(midi_control_mod_idx2.tag().into())
        .wrap();

    // Oscillator 2
    let sine2 = SineOsc::new().hz(modulator_osc2.tag().into()).wrap();
    let saw2 = SawOsc::new().hz(midi_pitch.tag().into()).wrap();
    let square2 = SquareOsc::new().hz(midi_pitch.tag().into()).wrap();
    let triangle2 = TriangleOsc::new().hz(midi_pitch.tag().into()).wrap();
    
    let midi_control_mod_hz1 = MidiControl::new(42, 0, 0.0, 440.0, 1760.0).wrap();
    let midi_control_mod_idx1 = MidiControl::new(43, 0, 0.0, 4.0, 16.0).wrap();

    let modulator_osc1 = Modulator::new(sine2.tag())
        .base_hz(midi_pitch.tag().into())
        .mod_hz(midi_control_mod_hz1.tag().into())
        .mod_idx(midi_control_mod_idx1.tag().into())
        .wrap();

    // Oscillator 1
    let sine1 = SineOsc::new().hz(modulator_osc1.tag().into()).wrap();
    let saw1 = SawOsc::new().hz(midi_pitch.tag().into()).wrap();
    let square1 = SquareOsc::new().hz(midi_pitch.tag().into()).wrap();
    let triangle1 = TriangleOsc::new().hz(midi_pitch.tag().into()).wrap();

    let sub1 = SquareOsc::new().hz(midi_pitch.tag().into()).wrap();
    let sub2 = SquareOsc::new().hz(midi_pitch.tag().into()).wrap();

    // Noise
    let noise = WhiteNoise::new().wrap();

    // Mixers
    let mut mixer = Mixer::new(vec![
        sine1.tag(),
        square1.tag(),
        saw1.tag(),
        triangle1.tag(),
        noise.tag(),
    ]);

    let midi_control_mix1 = MidiControl::new(32, 127, 0.0, 0.5, 1.0).wrap();
    let midi_control_mix2 = MidiControl::new(33, 0, 0.0, 0.5, 1.0).wrap();
    let midi_control_mix3 = MidiControl::new(34, 0, 0.0, 0.5, 1.0).wrap();
    let midi_control_mix4 = MidiControl::new(35, 0, 0.0, 0.5, 1.0).wrap();
    let midi_control_mix5 = MidiControl::new(36, 0, 0.0, 0.5, 1.0).wrap();

    mixer.levels = vec![
        midi_control_mix1.tag().into(),
        midi_control_mix2.tag().into(),
        midi_control_mix3.tag().into(),
        midi_control_mix4.tag().into(),
        midi_control_mix5.tag().into(),
    ];
    mixer.level = adsr.tag().into();

    // Filter
    let midi_control_cutoff = MidiControl::new(40, 127, 10.0, 1320.0, 20000.0).wrap();
    let midi_control_resonance = MidiControl::new(41, 0, 0.707, 4.0, 10.0).wrap();

    let low_pass_filter = Lpf::new(mixer.tag())
        .cutoff_freq(midi_control_cutoff.tag().into())
        .q(midi_control_resonance.tag().into())
        .wrap();

    // VCA
    let midi_control_volume = MidiControl::new(47, 64, 0.0, 0.5, 1.0).wrap();
    let vca = Vca::new(low_pass_filter.tag())
        .level(midi_control_volume.tag().into())
        .wrap();

    let graph = Rack::new(vec![
        midi_pitch.clone(),
        midi_control_mix1.clone(),
        midi_control_mix2.clone(),
        midi_control_mix3.clone(),
        midi_control_mix4.clone(),
        midi_control_mix5.clone(),
        midi_control_release.clone(),
        midi_control_cutoff.clone(),
        midi_control_resonance.clone(),
        midi_control_mod_hz1.clone(),
        midi_control_mod_hz2.clone(),
        midi_control_mod_idx1.clone(),
        midi_control_mod_idx2.clone(),
        midi_control_tri_lfo_hz.clone(),
        midi_control_volume.clone(),
        adsr,
        sine1,
        saw1,
        square1,
        triangle1,
        sub1,
        sub2,
        sine2,
        saw2,
        square2,
        triangle2,
        modulator_osc1,
        modulator_osc2,
        noise,
        tri_lfo,
        square_lfo,
        arc(mixer),
        low_pass_filter,
        vca,
    ]);

    Synth {
        midi: Midi {
            midi_pitch,
            midi_controls: vec![
                midi_control_mix1,
                midi_control_mix2,
                midi_control_mix3,
                midi_control_mix4,
                midi_control_mix5,
                midi_control_release,
                midi_control_cutoff,
                midi_control_mod_hz1,
                midi_control_mod_hz2,
                midi_control_mod_idx1,
                midi_control_mod_idx2,
                midi_control_resonance,
                midi_control_tri_lfo_hz,
                midi_control_volume,
            ],
        },
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
                set_step(synth.midi.midi_pitch.clone(), midi_step);
                on(&synth.voice, adsr_tag);
            } else if message[0] == 128 {
                off(&synth.voice, adsr_tag);
            } else if message[0] == 176 {
                for c in &synth.midi.midi_controls {
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

    if shifted_scope_data.len() >= 600 {
        let shifted_scope_data = shifted_scope_data[0..600].iter();
        let scope_points = shifted_scope_data
            .zip((0..600).into_iter())
            .map(|(y, x)| pt2(x as f32, y * 120.));

        draw.path()
            .stroke()
            .weight(2.)
            .points(scope_points)
            .color(CORNFLOWERBLUE)
            .x_y(-200., 0.);

        draw.to_frame(app, &frame).unwrap();
    }
}
