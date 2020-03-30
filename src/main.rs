use nannou::prelude::*;
use nannou_audio as audio;
use nannou_audio::Buffer;
use crossbeam::crossbeam_channel::unbounded;
use crossbeam::crossbeam_channel::Sender;
use crossbeam::crossbeam_channel::Receiver;
use std::f64::consts::PI;
use core::time::Duration;

fn main() {
    nannou::app(model).update(update).run();
}

struct Model {
    stream: audio::Stream<Synth>,
    receiver: Receiver<f32>,
    amps: Vec<f32>
}

struct Synth {
    voices: Vec<Oscillator>,
    sender: Sender<f32>
}

enum Waveshape {
    Sine,
    Square,
    Saw,
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

    fn saw_wave(&mut self) -> f32 {
        fn saw(p: f64) -> f32 {
            let q = p % (2. * PI);
            let n = (2. * q / 3.) as f32;
            if q <= (1.5 * PI) { n } else { 2. - n }
        }
        saw(2. * PI * self.phase)
    }

    // fn triangle_wave(&mut self) -> f32 {
    //     let saw_amp = self.saw_wave();

    // }

    fn sample(&mut self, sample_rate: f64) -> f32 {
        let amp = match self.shape {
            Waveshape::Sine => self.sine_wave(),
            Waveshape::Square => self.square_wave(),
            Waveshape::Saw => self.saw_wave(),
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
        Oscillator {
            phase: 0.0,
            hz: 440.,
            volume: 0.5,
            shape: Waveshape::Saw,
        },
        // Oscillator {
        //     phase: 0.0,
        //     hz: 261.63,
        //     volume: 0.5,
        //     shape: Waveshape::Sine,
        // },
        // Oscillator {
        //     phase: 0.0,
        //     hz: 155.56,
        //     volume: 0.5,
        //     shape: Waveshape::Sine,
        // },
        // Oscillator {
        //     phase: 0.0,
        //     hz: 196.00,
        //     volume: 0.5,
        //     shape: Waveshape::Sine,
        // },
        // Oscillator {
        //     phase: 0.0,
        //     hz: 261.63,
        //     volume: 0.5,
        //     shape: Waveshape::Sine,
        // },
    ];
    let model = Synth { voices, sender };
    let stream = audio_host
        .new_output_stream(model)
        .render(audio)
        .build()
        .unwrap();
    Model { stream, receiver, amps: vec![] }
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
                        voice.hz += 10.0;
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
                        voice.hz -= 10.0;
                    }
                })
                .unwrap();
        }
        _ => {}
    }
}

fn update(_app: &App, model: &mut Model, _update: Update) {
    let v: Vec<f32> = model.receiver.try_iter().collect();
    model.amps = v;
}

fn view(app: &App, model: &Model, frame: Frame) {
    frame.clear(BLACK);

    let draw = app.draw();

    let mut points: Vec<Point2> = vec![];

    for (i, amp) in model.amps.iter().enumerate() {
        points.push(pt2(i as f32, *amp * 100.));
    }

    draw.path().stroke().weight(1.).points(points);

    draw.to_frame(app, &frame).unwrap();
}