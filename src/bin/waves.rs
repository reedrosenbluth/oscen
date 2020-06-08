use core::cmp::Ordering;
use core::time::Duration;
use crossbeam::crossbeam_channel::{unbounded, Receiver, Sender};
use nannou::{prelude::*, ui::prelude::*};
use nannou_audio as audio;
use nannou_audio::Buffer;
use pitch_calc::calc::hz_from_step;
use std::thread;
use swell::envelopes::{off, on, Adsr};
use swell::signal::{arc, ArcMutex, Rack, Real, Signal, Tag};
use swell::midi::{listen_midi, MidiControl, MidiPitch};
use swell::operators::{Union, Vca, Lerp};
use swell::oscillators::{SineOsc, TriangleOsc, square_wave};
use swell::shaping::{SineFold, Tanh};

fn main() {
    nannou::app(model).update(update).run();
}

struct Model {
    ui: Ui,
    stream: audio::Stream<Synth>,
    receiver: Receiver<f32>,
    amps: Vec<f32>,
    max_amp: f32,
}

#[derive(Clone)]
struct Midi {
    midi_pitch: ArcMutex<MidiPitch>,
    midi_controls: Vec<ArcMutex<MidiControl>>,
}

struct Synth {
    midi: ArcMutex<Midi>,
    midi_receiver: Receiver<Vec<u8>>,
    voice: Rack,
    adsr_tag: Tag,
    union_tag: Tag,
    sender: Sender<f32>,
}

fn build_synth(midi_receiver: Receiver<Vec<u8>>, sender: Sender<f32>) -> Synth {
    //  Midi
    let midi_pitch = MidiPitch::wrapped();
    let midi_volume = MidiControl::wrapped(1);

    // Envelope Generator
    let adsr = Adsr::new(0.05, 0.05, 1.0, 0.2);
    let adsr_tag = adsr.tag();

    let sine = SineOsc::with_hz(midi_pitch.tag().into());
    let sinefold = SineFold::new(sine.tag());
    let tri = TriangleOsc::with_hz(midi_pitch.tag().into());
    let mut lerp = Lerp::new(sine.tag(), tri.tag());
    lerp.alpha = (0.2).into();
    let tanh = Tanh::new(sine.tag());
    let mut sq = square_wave(16, true);
    sq.hz = midi_pitch.tag().into();
    let mut union = Union::new(vec![sine.tag(), sinefold.tag(), sq.tag(), tanh.tag()]);
    union.level = adsr.tag().into();
    let union_tag = union.tag();
    let vca = Vca::wrapped(union_tag, (0.5).into());
    let graph = Rack::new(vec![
        midi_pitch.clone(),
        midi_volume.clone(),
        arc(adsr),
        arc(sine),
        arc(sinefold),
        arc(tri),
        arc(sq),
        arc(tanh),
        arc(union),
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
        union_tag,
        sender,
    }
}

fn model(app: &App) -> Model {
    let (sender, receiver) = unbounded();
    let (midi_sender, midi_receiver) = unbounded();

    thread::spawn(|| match listen_midi(midi_sender) {
        Ok(_) => (),
        Err(err) => println!("Error: {}", err),
    });

    // Create a window to receive key pressed events.
    app.set_loop_mode(LoopMode::Rate {
        update_interval: Duration::from_millis(1),
    });

    let _window = app
        .new_window()
        .size(900, 520)
        .key_pressed(key_pressed)
        .view(view)
        .build()
        .unwrap();

    let ui = app.new_ui().build().unwrap();

    let audio_host = audio::Host::new();
    let synth = build_synth(midi_receiver, sender);
    let stream = audio_host
        .new_output_stream(synth)
        .render(audio)
        .build()
        .unwrap();

    Model {
        ui,
        stream,
        receiver,
        amps: vec![],
        max_amp: 0.,
    }
}

// A function that renders the given `Audio` to the given `Buffer`.
// In this case we play a simple sine wave at the audio's current frequency in `hz`.
fn audio(synth: &mut Synth, buffer: &mut Buffer) {
    let midi_messages: Vec<Vec<u8>> = synth.midi_receiver.try_iter().collect();
    let adsr_tag = synth.adsr_tag;
    for message in midi_messages {
        if message.len() == 3 {
            let step = message[1] as f32;
            if message[0] == 144 {
                &synth
                    .midi
                    .lock()
                    .unwrap()
                    .midi_pitch
                    .lock()
                    .unwrap()
                    .set_step(step);
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
        synth.sender.send(amp as f32).unwrap();
    }
}

fn key_pressed(_app: &App, model: &mut Model, key: Key) {
    let activate = |n| {
        model
            .stream
            .send(move |synth| {
                let union_tag = synth.union_tag;
                if let Some(v) = synth.voice.nodes[&union_tag]
                    .module
                    .lock()
                    .unwrap()
                    .as_any_mut()
                    .downcast_mut::<Union>()
                {
                    let tag = v.waves[n];
                    v.active = tag;
                }
            })
            .unwrap();
    };
    match key {
        // Pause or unpause the audio when Space is pressed.
        Key::Key0 => activate(0),
        Key::Key1 => activate(1),
        Key::Key2 => activate(2),
        Key::Key3 => activate(3),
        _ => {}
    }
}

fn update(_app: &App, model: &mut Model, _update: Update) {
    let amps: Vec<f32> = model.receiver.try_iter().collect();
    let clone = amps.clone();

    // find max amplitude in waveform
    let max = amps.iter().max_by(|x, y| {
        if x > y {
            Ordering::Greater
        } else {
            Ordering::Less
        }
    });

    // store if it's greater than the previously stored max
    if max.is_some() && *max.unwrap() > model.max_amp {
        model.max_amp = *max.unwrap();
    }

    model.amps = clone;
}

fn view(app: &App, model: &Model, frame: Frame) {
    let draw = app.draw();
    let c = rgb(9. / 255., 9. / 255., 44. / 255.);
    draw.background().color(c);
    if frame.nth() == 0 {
        draw.to_frame(app, &frame).unwrap()
    }
    let mut shifted: Vec<f32> = vec![];
    let mut iter = model.amps.iter().peekable();

    let mut i = 0;
    while iter.len() > 0 {
        let amp = iter.next().unwrap_or(&0.);
        if amp.abs() < 0.01 && **iter.peek().unwrap_or(&amp) > *amp {
            shifted = model.amps[i..].to_vec();
            break;
        }
        i += 1;
    }

    let l = 600;
    let mut points: Vec<Point2> = vec![];
    for (i, amp) in shifted.iter().enumerate() {
        if i == l {
            break;
        }
        points.push(pt2(i as f32, amp * 120.));
    }

    // only draw if we got enough info back from the audio thread
    if points.len() == 600 {
        draw.path()
            .stroke()
            .weight(2.)
            .points(points)
            .color(CORNFLOWERBLUE)
            .x_y(-200., 0.);

        draw.to_frame(app, &frame).unwrap();
    }

    // Draw the state of the `Ui` to the frame.
    model.ui.draw_to_frame(app, &frame).unwrap();
}
