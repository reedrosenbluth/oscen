use core::cmp::Ordering;
use core::time::Duration;
use crossbeam::crossbeam_channel::{unbounded, Receiver, Sender};
use derive_more::Constructor;
use nannou::prelude::*;
use nannou::ui::prelude::*;
use nannou_audio as audio;
use nannou_audio::Buffer;

use swell::*;

fn main() {
    // nannou::app(model).update(update).simple_window(view).run();
    nannou::app(model).update(update).run();
}

#[derive(Constructor)]
struct Model {
    ui: Ui,
    ids: Ids,
    fm_mult: i32,
    stream: audio::Stream<Synth>,
    receiver: Receiver<f32>,
    amps: Vec<f32>,
    max_amp: f32,
}

struct Ids {
    fm_hz: widget::Id,
}

#[derive(Constructor)]
struct Synth {
    voice: Option<Box<dyn Wave + Send>>,
    sender: Sender<f32>,
}

fn model(app: &App) -> Model {
    let (sender, receiver) = unbounded();

    // Create a window to receive key pressed events.
    app.set_loop_mode(LoopMode::Rate {
        update_interval: Duration::from_millis(1),
    });

    let _window = app
        .new_window()
        .key_pressed(key_pressed)
        .key_released(key_released)
        .view(view)
        .build()
        .unwrap();

    let mut ui = app.new_ui().build().unwrap();

    let ids = Ids {
        fm_hz: ui.generate_widget_id(),
    };

    let audio_host = audio::Host::new();

    let synth = Synth {
        voice: None,
        sender,
    };

    let stream = audio_host
        .new_output_stream(synth)
        .render(audio)
        .build()
        .unwrap();

    Model {
        ui,
        ids,
        fm_mult: 1,
        stream,
        receiver,
        amps: vec![],
        max_amp: 0.,
    }
}

// A function that renders the given `Audio` to the given `Buffer`.
// In this case we play a simple sine wave at the audio's current frequency in `hz`.
fn audio(synth: &mut Synth, buffer: &mut Buffer) {
    let sample_rate = buffer.sample_rate() as f64;
    match &mut synth.voice {
        Some(voice) => {
            for frame in buffer.frames_mut() {
                let mut amp = 0.;
                amp += voice.sample();
                voice.update_phase(sample_rate);
                for channel in frame {
                    *channel = amp;
                }
                synth.sender.send(amp).unwrap();
            }
        }
        None => {}
    }
}

fn create_voice(hz: f64) -> Option<Box<dyn Wave + Send>> {
    let carrier = Box::new(SineWave::new(hz, 0.5));
    let modulator = Box::new(SineWave::new(hz, 1.0));
    let vco = VCO {
        wave: carrier,
        cv: modulator,
        fm_mult: 1,
    };
    let env = ADSRWave::new(
        0.05, // attack
        0.5,  // decay
        0.,   // sustain_time
        0.8,  // sustain_level
        3.,   // release
    );
    let vca = VCA {
        wave: Box::new(vco),
        cv: Box::new(env),
    };

    Some(Box::new(vca))
}

fn key2hz(key: Key) -> f64 {
    match key {
        Key::A => 131., // C3
        Key::W => 139., // C#/Db3
        Key::S => 147., // D3
        Key::E => 156., // D#/Eb3
        Key::D => 165., // E3
        Key::F => 175., // F3
        Key::T => 185., // F#/Gb3
        Key::G => 196., // G3
        Key::Y => 208., // G#/Ab3
        Key::H => 220., // A3
        Key::U => 233., // A#/Bb3
        Key::J => 247., // B3
        Key::K => 262., // C4
        Key::O => 277., // C#/Db4
        Key::L => 294., // D4
        _ => 0.,
    }
}

fn key_pressed(_app: &App, model: &mut Model, key: Key) {
    model.max_amp = 0.;
    let change_hz = |i| {
        model
            .stream
            .send(move |synth| {
                let factor = 2.0.powf(i / 12.);
                match &mut synth.voice {
                    Some(voice) => voice.mul_hz(factor),
                    None => {}
                }
            })
            .unwrap();
    };
    match key {
        // Play synth while spacebar is pressed
        Key::Space => {
            model
                .stream
                .send(move |synth| {
                    if synth.voice.is_none() {
                        synth.voice = create_voice(440.);
                    }
                })
                .unwrap();
        }
        // Raise the frequency when the up key is pressed.
        Key::Up => change_hz(1.),
        Key::Down => change_hz(-1.),
        _ => {
            model
                .stream
                .send(move |synth| {
                    if synth.voice.is_none() {
                        synth.voice = create_voice(key2hz(key));
                    }
                })
                .unwrap();
        }
    }
}

fn key_released(_app: &App, model: &mut Model, key: Key) {
    match key {
        // Remove synth when spacebar is released
        _ => model
            .stream
            .send(move |synth| {
                synth.voice = None;
            })
            .unwrap(),
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

    //UI
    // let ui = &mut model.ui.set_widgets();

    // fn slider(val: f32, min: f32, max: f32) -> widget::Slider<'static, f32> {
    //     widget::Slider::new(val, min, max)
    //         .w_h(200.0, 30.0)
    //         .label_font_size(15)
    //         .rgb(0.3, 0.3, 0.3)
    //         .label_rgb(1.0, 1.0, 1.0)
    //         .border(0.0)
    // }

    // for value in slider(model.fm_mult as f32, 0., 12.)
    //     .top_left_with_margin(20.0)
    //     .label("FM Multiplier")
    //     .set(model.ids.fm_hz, ui)
    // {
    //     model.fm_mult = (value as f64).round() as i32;
    //     model
    //         .stream
    //         .send(move |synth| {
    //             synth.voice.set_fm_mult((value as f64).round() as i32);
    //         })
    //         .unwrap();
    // }
}

fn view(app: &App, model: &Model, frame: Frame) {
    let draw = app.draw();
    let c = rgb(9. / 255., 9. / 255., 44. / 255.);
    draw.background().color(c);
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
            .x_y(-300., 0.);
    }

    draw.to_frame(app, &frame).unwrap();

    // Draw the state of the `Ui` to the frame.
    model.ui.draw_to_frame(app, &frame).unwrap();
}
