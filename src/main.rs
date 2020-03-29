use nannou::prelude::*;
use nannou_audio as audio;
use nannou_audio::Buffer;
use std::f64::consts::PI;

fn main() {
    nannou::app(model).run();
}

struct Model {
    stream: audio::Stream<Synth>,
}

struct Synth {
    voices: [Oscillator; 4]
}

enum Waveshape {
    Sin,
    Square,
}

struct Oscillator {
    phase: f64,
    hz: f64,
    volume: f32,
    shape: Waveshape
}

impl Oscillator {
    fn adjust_phase(&mut self, sample_rate: f64) {
        self.phase += self.hz / sample_rate;
        self.phase %= sample_rate;
    }

    fn sine_wave(&mut self, sample_rate: f64) -> f32 {
        let amp = (2. * PI * self.phase).sin() as f32;
        self.adjust_phase(sample_rate);
        amp
    }

    fn square_wave(&mut self, sample_rate: f64) -> f32 {
        let sine_amp = self.sine_wave(sample_rate);
        if sine_amp > 0. {
            self.volume
        } else {
            -self.volume
        }
    }

    fn sample(&mut self, sample_rate: f64) -> f32 {
        match self.shape {
            Waveshape::Sin => self.sine_wave(sample_rate),
            Waveshape::Square => self.square_wave(sample_rate),
        }
    }
}

fn model(app: &App) -> Model {
    // Create a window to receive key pressed events.
    app.new_window()
        .key_pressed(key_pressed)
        .view(view)
        .build()
        .unwrap();
    // Initialise the audio API so we can spawn an audio stream.
    let audio_host = audio::Host::new();
    // Initialise the state that we want to live on the audio thread.
    let model = Synth {
        voices: [
            Oscillator {
                phase: 0.0,
                hz: 261.63,
                volume: 0.5,
                shape: Waveshape::Sin
            },
            Oscillator {
                phase: 0.0,
                hz: 155.56,
                volume: 0.5,
                shape: Waveshape::Sin
            },
            Oscillator {
                phase: 0.0,
                hz: 196.00,
                volume: 0.5,
                shape: Waveshape::Sin
            },
            Oscillator {
                phase: 0.0,
                hz: 261.63,
                volume: 0.5,
                shape: Waveshape::Sin
            },
        ]
    };
    let stream = audio_host
        .new_output_stream(model)
        .render(audio)
        .build()
        .unwrap();
    Model { stream }
}

// A function that renders the given `Audio` to the given `Buffer`.
// In this case we play a simple sine wave at the audio's current frequency in `hz`.
fn audio(synth: &mut Synth, buffer: &mut Buffer) {
    let sample_rate = buffer.sample_rate() as f64;
    for frame in buffer.frames_mut() {
        let mut amp = 0.;
        for voice in &mut synth.voices {
            amp += voice.sample(sample_rate) * voice.volume;
        }
        for channel in frame {
            *channel = amp / synth.voices.len() as f32;
        }
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
                    for voice in &mut synth.voices {
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
                    for voice in &mut synth.voices {
                        voice.hz -= 10.0;
                    }
                })
                .unwrap();
        }
        _ => {}
    }
}

fn view(_app: &App, _model: &Model, frame: Frame) {
    frame.clear(DIMGRAY);
}