# Oscen [![crates.io](https://img.shields.io/crates/v/oscen.svg)](https://crates.io/crates/oscen)

Oscen [oh-sin] is a library for building modular synthesizers in Rust.

It contains a collection of components frequently used in sound synthesis
such as oscillators, filters, and envelope generators. It lets you
connect (or patch) the output of one module into the input of another.

## Example

```Rust
use crossbeam::crossbeam_channel::{unbounded, Receiver, Sender};
use nannou::prelude::*;
use nannou_audio as audio;
use nannou_audio::Buffer;
use oscen::filters::Lpf;
use oscen::operators::Modulator;
use oscen::oscillators::{sine_osc, square_osc, Oscillator};
use oscen::signal::*;

fn main() {
    nannou::app(model).update(update).run();
}

struct Model {
    pub stream: audio::Stream<Synth>,
    receiver: Receiver<f32>,
    samples: Vec<f32>,
}

struct Synth {
    sender: Sender<f32>,
    rack: Rack,
}

fn model(app: &App) -> Model {
    let (sender, receiver) = unbounded();
    app.new_window().size(700, 360).view(view).build().unwrap();
    let audio_host = audio::Host::new();

    // Build the Synth.
    // A Rack is a collection of synth modules.
    let mut rack = Rack::new(vec![]);

    let modulator = Modulator::new(sine_osc, 440, 4, 2).rack(&mut rack);

    // Create a square wave oscillator and add it the the rack.
    let square = Oscillator::new(square_osc)
        .hz(modulator.tag())
        .arg(0.5)
        .rack(&mut rack);

    // Create a low pass filter whose input is the square wave.
    Lpf::new(square.tag()).cutoff_freq(440).rack(&mut rack);

    let synth = Synth { sender, rack };
    let stream = audio_host
        .new_output_stream(synth)
        .render(audio)
        .build()
        .unwrap();

    Model {
        stream,
        receiver,
        samples: vec![],
    }
}

fn audio(synth: &mut Synth, buffer: &mut Buffer) {
    let sample_rate = buffer.sample_rate() as Real;
    for frame in buffer.frames_mut() {
        // The signal method returns the sample of the last synth module in
        // the rack.
        let amp = synth.rack.signal(sample_rate) as f32;

        for channel in frame {
            *channel = amp;
        }
        synth.sender.send(amp).unwrap();
    }
}

fn update(_app: &App, model: &mut Model, _update: Update) {
    let samples: Vec<f32> = model.receiver.try_iter().collect();
    model.samples = samples;
}

fn view(app: &App, model: &Model, frame: Frame) {
    use nannou_apps::scope;
    scope(app, &model.samples, frame);
}
```
