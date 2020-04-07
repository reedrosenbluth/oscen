use core::cmp::Ordering;
use core::time::Duration;
use crossbeam::crossbeam_channel::{unbounded, Receiver, Sender};
use derive_more::Constructor;
use nannou::prelude::*;
use nannou_audio as audio;
use nannou_audio::Buffer;

use swell::*;

fn main() {
    nannou::app(model).update(update).run();
}

#[derive(Constructor)]
struct Model {
    stream: audio::Stream<Synth>,
    receiver: Receiver<f32>,
    amps: Vec<f32>,
    max_amp: f32,
    wave_index: usize,
    num_waves: usize,
}

#[derive(Constructor)]
struct Synth {
    voice: Box<AvgWave>,
    sender: Sender<f32>,
}

fn model(app: &App) -> Model {
    const HZ: f64 = 440.;
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
    let square = WeightedWave(Box::new(SquareWave::new(HZ, 1.0)), 0.0);
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
    let waves = AvgWave {
        waves: vec![sine, square, saw, triangle, lerp, vca, vco],
    };
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
    Model {
        stream,
        receiver,
        amps: vec![],
        max_amp: 0.,
        wave_index: 0,
        num_waves,
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
        Key::Right => {
            model.wave_index += 1;
            model.wave_index %= model.num_waves;
            let mut ws = vec![0.; model.num_waves];
            ws[model.wave_index] = 1.0;
            model
                .stream
                .send(move |synth| {
                    synth.voice.set_weights(ws);
                })
                .unwrap();
        }
        Key::Left => {
            if model.wave_index == 0 {
                model.wave_index = model.num_waves - 1;
            } else {
                model.wave_index -= 1;
            }
            let mut ws = vec![0.; model.num_waves];
            ws[model.wave_index] = 1.0;
            model
                .stream
                .send(move |synth| {
                    synth.voice.set_weights(ws);
                })
                .unwrap();
        }
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

        draw.to_frame(app, &frame).unwrap();
    }
}
