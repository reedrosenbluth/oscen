use core::cmp::Ordering;
use core::time::Duration;
use crossbeam::crossbeam_channel::{unbounded, Receiver, Sender};
use nannou::{prelude::*, ui::prelude::*};
use nannou_audio as audio;
use nannou_audio::Buffer;
use std::thread;
use swell::instruments;
use swell::instruments::WaveGuide;
use swell::midi::{listen_midi, MidiControl, MidiPitch};
use swell::oscillators::SquareOsc;
use swell::signal::{ArcMutex, Builder, Rack, Real, Signal, Tag};

fn main() {
    nannou::app(model).update(update).run();
}

#[allow(dead_code)]
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
    midi: Midi,
    midi_receiver: Receiver<Vec<u8>>,
    rack: Rack,
    karplus_tag: Tag,
    sender: Sender<f32>,
}

fn build_synth(midi_receiver: Receiver<Vec<u8>>, sender: Sender<f32>) -> Synth {
    let mut rack = Rack::new(vec![]);
    //  Midi
    let midi_pitch = MidiPitch::new().wrap();
    rack.append(midi_pitch.clone());
    let midi_volume = MidiControl::new(1, 64, 0.0, 0.5, 1.0).wrap();
    rack.append(midi_volume.clone());

    let excite = SquareOsc::new().hz(110).wrap();
    rack.append(excite.clone());

    let karplus = WaveGuide::new(excite.tag())
        .hz(midi_pitch.tag())
        .wet_decay(0.95)
        .attack(0.005)
        .release(0.005)
        .wrap();
    let karplus_tag = karplus.tag();
    rack.append(karplus);

    Synth {
        midi: Midi {
            midi_pitch,
            midi_controls: vec![midi_volume],
        },
        midi_receiver,
        rack,
        karplus_tag,
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

    let _window = app.new_window().size(900, 520).view(view).build().unwrap();

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
    let string_tag = synth.karplus_tag;
    for message in midi_messages {
        if message.len() == 3 {
            let step = message[1] as f32;
            if message[0] == 144 {
                synth.midi.midi_pitch.lock().unwrap().step(step);
                instruments::on(&synth.rack, string_tag);
            } else if message[0] == 128 {
                instruments::off(&synth.rack, string_tag);
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
        let amp = synth.rack.signal(sample_rate) as f32;
        for channel in frame {
            *channel = amp;
        }
        synth.sender.send(amp).unwrap();
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
