use crossbeam::crossbeam_channel::{unbounded, Receiver, Sender};
use nannou::prelude::*;
use nannou_audio as audio;
use nannou_audio::Buffer;
use oscen::ops::*;
use oscen::osc::*;
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
    outputs: Box<Outputs>,
    union: Box<Union>,
    names: Vec<&'static str>,
}

fn model(app: &App) -> Model {
    let (sender, receiver) = unbounded();
    let mut names = vec![];
    app.new_window()
        .key_pressed(key_pressed)
        .size(700, 500)
        .view(view)
        .build()
        .unwrap();
    let audio_host = audio::Host::new();

    // Build the Synth.
    // A Rack is a collection of synth modules.
    let mut rack = Rack::new();
    let mut controls = Controls::new();
    let outputs = Outputs::new();
    let mut oscs = vec![];
    let freq = 220;

    // Sine
    let sine = OscBuilder::new(sine_osc)
        .hz(freq)
        .rack(&mut rack, &mut controls);
    oscs.push(sine.tag());
    names.push("Sine");

    // Square
    let square = OscBuilder::new(square_osc)
        .hz(freq)
        .rack(&mut rack, &mut controls);
    oscs.push(square.tag());
    names.push("Square");

    // Saw
    let saw = OscBuilder::new(saw_osc)
        .hz(freq)
        .rack(&mut rack, &mut controls);
    oscs.push(saw.tag());
    names.push("Saw");

    // Triangle
    let tri = OscBuilder::new(triangle_osc)
        .hz(freq)
        .rack(&mut rack, &mut controls);
    oscs.push(tri.tag());
    names.push("Triangle");

    // Fourier Square 8.
    let mut builder = square_wave(8);
    builder.hz(freq);
    let sq8 = builder.rack(&mut rack, &mut controls);
    oscs.push(sq8.tag());
    names.push("Fourier Square 8");

    // Fourier tri 8.
    let mut builder = triangle_wave(8);
    builder.hz(freq);
    let tri8 = builder.rack(&mut rack, &mut controls);
    oscs.push(tri8.tag());
    names.push("Fourier Triangle 8");

    // WhiteNoise
    let wn = WhiteNoiseBuilder::new()
        .amplitude(0.5)
        .rack(&mut rack, &mut controls);
    oscs.push(wn.tag());
    names.push("White Noise");

    // PinkNoise
    let pn = PinkNoiseBuilder::new()
        .amplitude(0.5)
        .rack(&mut rack, &mut controls);
    oscs.push(pn.tag());
    names.push("Pink Noise");

    // Mixer
    let mix = MixerBuilder::new(vec![sine.tag(), square.tag()]).rack(&mut rack);
    oscs.push(mix.tag());
    names.push("Mixer Sine & Square");

    // Product
    let prod = ProductBuilder::new(vec![sine.tag(), pn.tag()]).rack(&mut rack);
    oscs.push(prod.tag());
    names.push("Product Sine & Square");

    // LFO
    let lfo = OscBuilder::new(sine_osc)
        .hz(1)
        .rack(&mut rack, &mut controls);
    oscs.push(lfo.tag());
    names.push("LFO sine");

    // Vca
    let vca = VcaBuilder::new(sine.tag())
        .level(Control::V(In::Cv(lfo.tag(), 0)))
        .rack(&mut rack, &mut controls);
    oscs.push(vca.tag());
    names.push("Vca amp contolled by sine");

    // CrossFade
    let cf = CrossFadeBuilder::new(sine.tag(), square.tag()).rack(&mut rack, &mut controls);
    cf.set_alpha(&mut controls, Control::V(In::Cv(lfo.tag(), 0)));
    oscs.push(cf.tag());
    names.push("CrossFade Sine & Square, alpha is sine lfo");

    let union = UnionBuilder::new(oscs).rack(&mut rack, &mut controls);

    let synth = Synth {
        sender,
        rack,
        controls: Box::new(controls),
        outputs: Box::new(outputs),
        union,
        names,
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
    let sample_rate = buffer.sample_rate() as Real;
    for frame in buffer.frames_mut() {
        let amp = synth
            .rack
            .mono(&mut synth.controls, &mut synth.outputs, sample_rate);

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
    if frame.nth() == 0 {
        println!("Active module: 0 - Sine");
    };
    use nannou_apps::scope;
    scope(app, &model.samples, frame);
}

fn key_pressed(_app: &App, model: &mut Model, key: Key) {
    match key {
        // Pause or unpause the audio when Space is pressed.
        Key::Space => {
            model
                .stream
                .send(|synth| {
                    let active = synth.union.active(&synth.controls, &synth.outputs);
                    let n = synth.rack.num_modules() - 1;
                    println!(
                        "Active module: {} - {}",
                        (active + 1) % n,
                        synth.names[(active + 1) % n]
                    );
                    synth
                        .union
                        .set_active(&mut synth.controls, Control::I((active + 1) % n));
                })
                .unwrap();
        }
        // Raise the frequency when the up key is pressed.
        _ => {}
    }
}
