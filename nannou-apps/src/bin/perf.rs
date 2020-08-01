use nannou::prelude::*;
use nannou_audio as audio;
use nannou_audio::Buffer;
use oscen::oscillators::*;
use oscen::operators::Mixer;
use oscen::signal::*;

fn main() {
    nannou::app(model).run();
}

struct Model {
    stream: audio::Stream<Rack>,
}

fn model(app: &App) -> Model {
    app.new_window().size(250, 250).build().unwrap();
    let audio_host = audio::Host::new();
    let mut rack = Rack::new(vec![]);
    let num_oscillators = 100;
    let amp = 1.0 / num_oscillators as f64;
    let mut oscs = vec![];
    for _ in 0..num_oscillators {
        let osc = Oscillator::new(square_osc)
            .amplitude(amp)
            .hz(200)
            .arg(0.5)
            .rack(&mut rack);
        oscs.push(osc.tag());
    }
    Mixer::new(oscs).level(0.2).rack(&mut rack);
    let stream = audio_host
        .new_output_stream(rack)
        .render(audio)
        .build()
        .unwrap();
    Model { stream }
}

fn audio(rack: &mut Rack, buffer: &mut Buffer) {
    let sample_rate = buffer.sample_rate() as Real;
    for frame in buffer.frames_mut() {
        let amp = rack.signal(sample_rate) as f32;
        for channel in frame {
            *channel = amp;
        }
    }
}
