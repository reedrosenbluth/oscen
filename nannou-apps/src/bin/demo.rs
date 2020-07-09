use crossbeam::crossbeam_channel::{unbounded, Receiver, Sender};
use nannou::prelude::*;
use nannou_audio as audio;
use nannou_audio::Buffer;
use swell::filters::Lpf;
use swell::oscillators::SquareOsc;
use swell::signal::*;

fn main() {
    nannou::app(model).update(update).run();
}

#[allow(dead_code)]
struct Model {
    stream: audio::Stream<Synth>,
    receiver: Receiver<f32>,
    amps: Vec<f32>,
}

struct Synth {
    sender: Sender<f32>,
    rack: Rack,
}

fn model(app: &App) -> Model {
    let (sender, receiver) = unbounded();
    let _window = app.new_window().size(700, 360).view(view).build().unwrap();
    let audio_host = audio::Host::new();
    
    let mut rack = Rack::new(vec![]);
    let square = SquareOsc::new().hz(220).rack(&mut rack);
    Lpf::new(square.tag()).cutoff_freq(440).rack(&mut rack);
    let synth = Synth { sender, rack};

    let stream = audio_host
        .new_output_stream(synth)
        .render(audio)
        .build()
        .unwrap();

    Model {
        stream,
        receiver,
        amps: vec![],
    }
}

// A function that renders the given `Audio` to the given `Buffer`.
fn audio(synth: &mut Synth, buffer: &mut Buffer) {
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
    model.amps = amps;
}

fn view(app: &App, model: &Model, frame: Frame) {
    use nannou_apps::scope;
    scope(app, &model.amps, frame);
}