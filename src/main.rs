use core::cmp::Ordering;
use core::time::Duration;
use crossbeam::crossbeam_channel::{unbounded, Receiver, Sender, TryIter};
use math::round::floor;
use nannou::prelude::*;
use nannou_audio as audio;
use nannou_audio::Buffer;
use std::f64::consts::PI;

fn main() {
    nannou::app(model).update(update).run();
}

const WAVE_TABLE_LEN: usize = 2048;
type Wavetable = [f64; WAVE_TABLE_LEN];
type Sprite = Vec<Wavetable>; // Ableton calls collections of Wavetables "sprites"

struct Model {
    stream: audio::Stream<Synth>,
    receiver: Receiver<f32>,
    amps: Vec<f32>,
    max_amp: f32,
}

struct Synth {
    voices: Vec<Oscillator>,
    sender: Sender<f32>,
    sprite: Sprite,
}

#[derive(Copy, Clone)]
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
    sprite_position: i32,
}

impl Oscillator {
    fn new(phase: f64, hz: f64, volume: f32, shape: Waveshape) -> Self {
        Oscillator {
            phase: phase,
            hz: hz,
            volume: volume,
            shape: shape,
            sprite_position: 0,
        }
    }

    fn sine_wave(&mut self) -> f32 {
        (2. * PI * self.phase).sin() as f32
    }

    fn square_wave(&mut self) -> f32 {
        let sine_amp = self.sine_wave();
        if sine_amp > 0. {
            self.volume
        } else {
            -self.volume
        }
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

    fn sample(&mut self, sample_rate: f64, sprite: &Sprite) -> f32 {
        // let amp = match self.shape {
        //     Waveshape::Sine => self.sine_wave(),
        //     Waveshape::Square => self.square_wave(),
        //     Waveshape::Ramp => self.ramp_wave(),
        //     Waveshape::Saw => self.saw_wave(),
        //     Waveshape::Triangle => self.triangle_wave(),
        // };

        // self.phase += self.hz / sample_rate;
        // self.phase %= sample_rate;

        // Find wavetable in sprite based on sprite_position
        // TODO: Somehow interpolate between different wavetables? Would be cool
        let wavetable = sprite[self.sprite_position as usize];

        // Get sample point from wavetable
        // TODO: Polynomial interpolation
        // Linear interpolation:
        // p(x) = f(x0) + (f(x1) - f(x0)) / (x1 - x0) * (x - x0);
        let x0 = self.phase.floor();
        let x1 = self.phase.ceil();

        let y0 = wavetable[x0 as usize];
        let y1 = wavetable[x1 as usize % WAVE_TABLE_LEN];
        let amp = interpolate(self.phase, x0, y0, x1, y1);

        // Update phase accumulator
        self.phase += WAVE_TABLE_LEN as f64 * self.hz / sample_rate;
        self.phase %= WAVE_TABLE_LEN as f64;

        amp as f32
    }
}

fn interpolate(x: f64, x0: f64, y0: f64, x1: f64, y1: f64) -> f64 {
    y0 + (y1 - y0) / (x1 - x0) * (x - x0)
}

fn gen_table(waveshape: Waveshape) -> Wavetable {
    fn gen_sin(t: f64) -> f64 {
        (2. * PI * t).sin()
    }
    fn gen_sqr(t: f64) -> f64 {
        2. * (2. * t.floor() - (2. * t).floor()) + 1.
    }
    fn gen_saw(t: f64) -> f64 {
        2. * (t - (0.5 + t).floor())
    }
    fn gen_tri(t: f64) -> f64 {
        2. * (2. * (t - (t + 0.5).floor())).abs() - 1.
    }

    let wave_func = match waveshape {
        Waveshape::Sine => gen_sin,
        Waveshape::Square => gen_sqr,
        Waveshape::Saw | Waveshape::Ramp => gen_saw,
        Waveshape::Triangle => gen_tri,
    };

    let mut table = [0.; WAVE_TABLE_LEN];
    for i in 0..WAVE_TABLE_LEN {
        let t = i as f64 / WAVE_TABLE_LEN as f64;
        table[i] = wave_func(t);
    }
    table
}

fn model(app: &App) -> Model {
    let (sender, receiver) = unbounded();

    // Create a window to receive key pressed events.
    app.set_loop_mode(LoopMode::Rate {
        update_interval: Duration::from_millis(1),
    });
    app.new_window()
        .key_pressed(key_pressed)
        .view(view)
        .size(1536, 768)
        .build()
        .unwrap();

    // Initialize the audio API so we can spawn an audio stream.
    let audio_host = audio::Host::new();

    // Initialize the state that we want to live on the audio thread.
    let voices = vec![
        Oscillator::new(0.0, 220.00, 0.5, Waveshape::Sine),
        // Oscillator::new(0.0, 130.81, 0.5, Waveshape::Sine),
        // Oscillator::new(0.0, 155.56, 0.5, Waveshape::Sine),
        // Oscillator::new(0.0, 196.00, 0.5, Waveshape::Sine),
    ];
    let model = Synth {
        voices,
        sender,
        sprite: vec![
            gen_table(Waveshape::Sine),
            gen_table(Waveshape::Square),
            gen_table(Waveshape::Saw),
            gen_table(Waveshape::Triangle),
        ],
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
    }
}

// A function that renders the given `Audio` to the given `Buffer`.
// In this case we play a simple sine wave at the audio's current frequency in `hz`.
fn audio(synth: &mut Synth, buffer: &mut Buffer) {
    let sample_rate = buffer.sample_rate() as f64;
    for frame in buffer.frames_mut() {
        let mut amp = 0.;
        for voice in synth.voices.iter_mut() {
            amp += voice.sample(sample_rate, &synth.sprite) * voice.volume;
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
        // Increase sprite position when the right key is pressed.
        Key::Right => {
            model
                .stream
                .send(|synth| {
                    for voice in synth.voices.iter_mut() {
                        voice.sprite_position += 1;
                        voice.sprite_position %= synth.sprite.len() as i32;
                    }
                })
                .unwrap();
        }
        // Increase sprite position when the right key is pressed.
        Key::Left => {
            model
                .stream
                .send(|synth| {
                    for voice in synth.voices.iter_mut() {
                        voice.sprite_position -= 1;
                        if voice.sprite_position < 0 {
                            voice.sprite_position = synth.sprite.len() as i32 - 1;
                        }
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

    // I think the amp.abs() < 0.01 is messing up square waves. But also,
    // I don't really understand how this code works lol
    let mut i = 0;
    while iter.len() > 0 {
        let amp = iter.next().unwrap();
        // if amp.abs() < 0.01 && **iter.peek().unwrap() > *amp {
        if iter.peek().is_some() && **iter.peek().unwrap() > *amp {
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
        points.push(pt2(i as f32, amp * 80.));
    }

    // only draw if we got enough info back from the audio thread
    if points.len() == 600 {
        let draw = app.draw();
        frame.clear(BLACK);
        draw.path()
            .stroke()
            .weight(1.)
            .points(points)
            .x_y(-300., 0.);

        draw.to_frame(app, &frame).unwrap();
    }
}
