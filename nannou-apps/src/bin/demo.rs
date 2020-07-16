use crossbeam::crossbeam_channel::{unbounded, Receiver, Sender};
use nannou::prelude::*;
use nannou_audio as audio;
use nannou_audio::Buffer;
use std::cell::RefCell;
use swell::filters::Lpf;
use swell::operators::Modulator;
use swell::oscillators::{SineOsc, SquareOsc};
use swell::signal::*;

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

    // Use a low frequencey sine wave to modulate the frequency of a square wave.
    let sine = SineOsc::new().hz(1).wrap();
    let r = rack_append(sine.clone());

    let modulator = Modulator::new(sine.tag())
        .base_hz(440)
        .mod_hz(220)
        .mod_idx(1)
        .wrap();
    let r = r.and_then(|()| rack_append(modulator.clone()));

    // Create a square wave oscillator and add it the the rack.
    let square = SquareOsc::new().hz(modulator.tag()).wrap();
    let r = r.and_then(|()| rack_append(square.clone()));

    // Create a low pass filter whose input is the square wave.
    let r = r.and_then(|()| rack_append(Lpf::new(square.tag()).cutoff_freq(880).wrap()));
    let state = r.and_then(|()| get_state());
    let refcell: RefCell<Environment> = (state.run)(RefCell::new(Environment::new(44_100.0)));
    let environment = refcell.into_inner();

    println!("{:?}", environment);

    let synth = Synth { sender, rack: environment.rack };
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
