use nannou::prelude::*;
use nannou_audio as audio;
use nannou_audio::Buffer;
use crossbeam::crossbeam_channel::{unbounded, Sender, Receiver, TryIter};
use core::cmp::Ordering;
use std::f64::consts::PI;
use core::time::Duration;
use math::round::floor;

fn main() {
    nannou::app(model).update(update).run();
}

struct Model {
    stream: audio::Stream<Synth>,
    receiver: Receiver<f32>,
    amps: Vec<f32>,
    max_amp: f32
}

struct Synth {
    voices: Vec<Oscillator>,
    sender: Sender<f32>
}

enum Waveshape {
    Sine,
    Square,
    Ramp,
    Saw,
    Triangle,
}

struct Oscillator {
    phase: f64,
    hz: f64,
    volume: f32,
    shape: Waveshape,
}

impl Oscillator {
    fn sine_wave(&mut self) -> f32 {
        (2. * PI * self.phase).sin() as f32
    }

    fn square_wave(&mut self) -> f32 {
        let sine_amp = self.sine_wave();
        if sine_amp > 0. { self.volume } else { -self.volume }
    }

    fn ramp_wave(&mut self) -> f32 {
        (2. * (self.phase - floor(0.5 + self.phase, 0))) as f32
    }

    fn saw_wave(&mut self) -> f32 {
        let t = self.phase - 0.5;
        (2. * (-t - floor(0.5 - t, 0))) as f32
    }

    fn triangle_wave(&mut self) -> f32 {
        let t = self.phase - 0.5 - 0.25;
        let saw_amp = (2. * (-t - floor(0.5 - t, 0))) as f32;
        2. * saw_amp.abs() - self.volume
    }

    fn sample(&mut self, sample_rate: f64) -> f32 {
        let amp = match self.shape {
            Waveshape::Sine => self.sine_wave(),
            Waveshape::Square => self.square_wave(),
            Waveshape::Ramp => self.ramp_wave(),
            Waveshape::Saw => self.saw_wave(),
            Waveshape::Triangle => self.triangle_wave(),
        };

        self.phase += self.hz / sample_rate;
        self.phase %= sample_rate;

        amp
    }
}

fn model(app: &App) -> Model {

    let (sender, receiver) = unbounded();

    // Create a window to receive key pressed events.
    app.set_loop_mode(LoopMode::Rate {
        update_interval: Duration::from_millis(1)
    });
    app.new_window()
        .key_pressed(key_pressed)
        .view(view)
        .build()
        .unwrap();
    // Initialise the audio API so we can spawn an audio stream.
    let audio_host = audio::Host::new();
    // Initialise the state that we want to live on the audio thread.
    let voices = vec![
        // Oscillator {
        //     phase: 0.0,
        //     hz: 220.00,
        //     volume: 0.5,
        //     shape: Waveshape::Sine,
        // },
        Oscillator {
            phase: 0.0,
            hz: 130.81,
            volume: 0.5,
            shape: Waveshape::Sine
        },
        Oscillator {
            phase: 0.0,
            hz: 155.56,
            volume: 0.5,
            shape: Waveshape::Sine,
        },
        Oscillator {
            phase: 0.0,
            hz: 196.00,
            volume: 0.5,
            shape: Waveshape::Sine,
        },
    ];
    let model = Synth { voices, sender };
    let stream = audio_host
        .new_output_stream(model)
        .render(audio)
        .build()
        .unwrap();
    Model { stream, receiver, amps: vec![], max_amp: 0. }
}

// A function that renders the given `Audio` to the given `Buffer`.
// In this case we play a simple sine wave at the audio's current frequency in `hz`.
fn audio(synth: &mut Synth, buffer: &mut Buffer) {
    let sample_rate = buffer.sample_rate() as f64;
    for frame in buffer.frames_mut() {
        let mut amp = 0.;
        for voice in synth.voices.iter_mut() {
            amp += voice.sample(sample_rate) * voice.volume;
        }
        for channel in frame {
            *channel = amp / synth.voices.len() as f32;
        }
        synth.sender.send(amp).unwrap();
    }
}



fn key_pressed(_app: &App, model: &mut Model, key: Key) {
    model.max_amp = 0.;
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
        Key::Up => {
            model
                .stream
                .send(|synth| {
                    for voice in synth.voices.iter_mut() {
                        let start_freq = voice.hz;
                        let new_freq = start_freq * (2.0.powf(1. / 12.));
                        voice.hz = new_freq;
                    }
                })
                .unwrap();
        }
        // Lower the frequency when the down key is pressed.
        Key::Down => {
            model
                .stream
                .send(|synth| {
                    for voice in synth.voices.iter_mut() {
                        let start_freq = voice.hz;
                        let new_freq = start_freq * (2.0.powf(1. / 12.).powf(-1.));
                        voice.hz = new_freq;
                    }
                })
                .unwrap();
        }
        _ => {}
    }
}

fn update(_app: &App, model: &mut Model, _update: Update) {
    let amps: Vec<f32> = model.receiver.try_iter().collect();
    model.amps = amps;
}

fn view(app: &App, model: &Model, frame: Frame) {
    let mut shifted: Vec<f32> = vec![];
    let mut iter = model.amps.iter().peekable();

    let mut i = 0;
    while iter.len() > 0 { 
        let amp = iter.next().unwrap();
        if amp.abs() < 0.01 && **iter.peek().unwrap() > *amp {
            shifted = model.amps[i..].to_vec();
            break;
        }
        i += 1;
    }


    let l = 600;
    let mut points: Vec<Point2> = vec![];
    for (i, amp) in shifted.iter().enumerate() {
        if i == l { break; }
        points.push(pt2(i as f32, amp * 80.));
    }

    // only draw if we got enough info back from the audio thread
    if points.len() == 600 {
        let draw = app.draw();
        frame.clear(BLACK);
        draw.path().stroke().weight(1.).points(points).x_y(-300., 0.);

        draw.to_frame(app, &frame).unwrap();
    }
}