use crossbeam::crossbeam_channel::{unbounded, Receiver, Sender};
use nannou::prelude::*;
use nannou_audio as audio;
use nannou_audio::Buffer;
use oscen::operators::ModulatorBuilder;
use oscen::oscillators::{sine_osc, triangle_osc, OscBuilder};
use oscen::rack::*;

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
    controls: Box<Controls>,
    state: Box<State>,
    outputs: Box<Outputs>,
}

fn model(app: &App) -> Model {
    let (sender, receiver) = unbounded();
    app.new_window().size(700, 360).view(view).build().unwrap();
    let audio_host = audio::Host::new();

    let (mut rack, mut controls, mut state, outputs) = tables();

    let modulator = ModulatorBuilder::new(sine_osc)
        .hz(220)
        .ratio(0.1)
        .index(2)
        .rack(&mut rack, &mut controls, &mut state);

    // Create a square wave oscillator and add it the the rack.
    let _triangle = OscBuilder::new(triangle_osc).hz(modulator.tag()).rack(
        &mut rack,
        &mut controls,
        &mut state,
    );

    let synth = Synth {
        sender,
        rack,
        controls,
        state,
        outputs,
    };
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
    let sample_rate = buffer.sample_rate() as f32;
    for frame in buffer.frames_mut() {
        // The signal method returns the sample of the last synth module in
        // the rack.
        let amp = synth.rack.mono(
            &synth.controls,
            &mut synth.state,
            &mut synth.outputs,
            sample_rate,
        );

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
