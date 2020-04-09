use core::cmp::Ordering;
use core::time::Duration;
use crossbeam::crossbeam_channel::{unbounded, Receiver, Sender};
use derive_more::Constructor;
use nannou::prelude::*;
use nannou::ui::prelude::*;
use nannou_audio as audio;
use nannou_audio::Buffer;

use swell::*;
use widget::toggle::TimesClicked;
use widget::Toggle;

fn main() {
    nannou::app(model).update(update).run();
}

#[derive(Constructor)]
struct Model {
    stream: audio::Stream<Synth>,
    receiver: Receiver<f32>,
    amps: Vec<f32>,
    max_amp: f32,
    ui: Ui,
    ids: Vec<widget::Id>,
    wave_indices: Vec<f32>,
}

#[derive(Constructor)]
struct Synth {
    voice: Box<PolyWave>,
    sender: Sender<f32>,
}

fn model(app: &App) -> Model {
    const HZ: f64 = 220.;
    let (sender, receiver) = unbounded();

    // Create a window to receive key pressed events.
    app.set_loop_mode(LoopMode::Rate {
        update_interval: Duration::from_millis(1),
    });
    app.new_window()
        .size(600, 340)
        .key_pressed(key_pressed)
        .view(view)
        .build()
        .unwrap();
    // Initialise the audio API so we can spawn an audio stream.
    let audio_host = audio::Host::new();
    // Initialise the state that we want to live on the audio thread.
    let sine = WeightedWave(Box::new(SineWave::new(HZ, 1.0)), 1.0);
    // let square = WeightedWave(Box::new(SquareWave::new(HZ, 1.0)), 0.0);
    let square = WeightedWave(Box::new(square_wave(16, HZ)), 0.0);
    let saw = WeightedWave(Box::new(SawWave::new(HZ, 1.0)), 0.0);
    let triangle = WeightedWave(Box::new(TriangleWave::new(HZ, 1.0)), 0.0);
    let lerp = WeightedWave(
        Box::new(LerpWave {
            wave1: Box::new(SineWave::new(HZ, 1.0)),
            wave2: Box::new(SquareWave::new(HZ, 1.0)),
            alpha: 0.5,
        }),
        0.0,
    );
    let vca = WeightedWave(
        Box::new(VCA {
            wave: Box::new(SineWave::new(2.0 * HZ, 1.0)),
            cv: Box::new(SineWave::new(HZ / 5.5, 1.0)),
        }),
        0.0,
    );
    let vco = WeightedWave(
        Box::new(VCO {
            wave: Box::new(SineWave::new(HZ, 1.0)),
            cv: Box::new(SineWave::new(HZ, 1.0)),
            fm_mult: 1,
        }),
        0.0,
    );

    let waves = PolyWave::new_normal(vec![sine, square, saw, triangle, lerp, vca, vco]);
    let num_waves = waves.waves.len();
    let model = Synth {
        voice: Box::new(waves),
        sender,
    };
    let stream = audio_host
        .new_output_stream(model)
        .render(audio)
        .build()
        .unwrap();

    let mut ui = app.new_ui().build().unwrap();
    let mut ids: Vec<widget::Id> = Vec::new();
    for _ in 0..num_waves {
        ids.push(ui.generate_widget_id());
    }

    let mut wave_indices = vec![0.; num_waves];
    wave_indices[0] = 1.0;

    Model {
        stream,
        receiver,
        amps: vec![],
        max_amp: 0.,
        ui,
        ids,
        wave_indices,
    }
}

// A function that renders the given `Audio` to the given `Buffer`.
// In this case we play a simple sine wave at the audio's current frequency in `hz`.
fn audio(synth: &mut Synth, buffer: &mut Buffer) {
    let sample_rate = buffer.sample_rate() as f64;
    for frame in buffer.frames_mut() {
        let mut amp = 0.;
        amp += synth.voice.sample();
        synth.voice.update_phase(sample_rate);
        for channel in frame {
            *channel = amp;
        }
        synth.sender.send(amp).unwrap();
    }
}

fn key_pressed(_app: &App, model: &mut Model, key: Key) {
    model.max_amp = 0.;
    let change_hz = |i| {
        model
            .stream
            .send(move |synth| {
                let factor = 2.0.powf(i / 12.);
                synth.voice.mul_hz(factor);
            })
            .unwrap();
    };
    match key {
        // Pause or unpause the audio when Space is pressed.
        Key::Space => {
            if model.stream.is_playing() {
                model.stream.pause().unwrap();
            } else {
                model.stream.play().unwrap();
            }
        }
        // Raise the frequency when the up key is pressed.
        Key::Up => change_hz(1.),
        Key::Down => change_hz(-1.),
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

    let ui = &mut model.ui.set_widgets();

    let labels = &["Sine", "Square(8)", "Saw", "Triangle", "Lerp", "AM", "FM"];

    fn toggle(onoff: bool, lbl: &'static str) -> Toggle<'static> {
        Toggle::new(onoff)
            .w_h(100.0, 25.0)
            .label(lbl)
            .label_font_size(15)
            .rgb(9. / 255., 9. / 255., 44. / 255.)
            .label_color(if onoff {
                ui::color::WHITE
            } else {
                ui::color::DARK_RED
            })
            .border(0.0)
    }
    let mut toggles: Vec<TimesClicked> = Vec::new();
    let flags: Vec<bool> = model.wave_indices.iter().map(|x| *x > 0.).collect();

    for (i, f) in flags.iter().enumerate() {
        toggles.push(
            toggle(*f, labels[i])
                .top_left_with_margins(75.0 + 25.0 * i as f64, 20.)
                // .down(10.)
                .set(model.ids[i], ui),
        );
    }

    for (i, e) in toggles.iter_mut().enumerate() {
        for c in e {
            if c {
                model.wave_indices[i] = 1.
            } else {
                model.wave_indices[i] = 0.
            }
        }
    }
    let ws = model.wave_indices.clone();
    model
        .stream
        .send(move |synth| {
            synth.voice.set_weights(ws);
            synth.voice.normalize_weights();
        })
        .unwrap();
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
            .x_y(-150., 0.);

        draw.to_frame(app, &frame).unwrap();
    }
    model.ui.draw_to_frame(app, &frame).unwrap();
}
