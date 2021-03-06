use crossbeam::crossbeam_channel::{unbounded, Receiver, Sender};
use nannou::prelude::*;
use nannou_audio as audio;
use nannou_audio::Buffer;
use oscen::envelopes::*;
use oscen::filters::*;
use oscen::operators::*;
use oscen::oscillators::*;
use oscen::rack::*;
use std::sync::Arc;

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
    buffers: Box<Buffers>,
    union: Arc<Union>,
    adsr: Arc<Adsr>,
    names: Vec<&'static str>,
}

fn model(app: &App) -> Model {
    let (sender, receiver) = unbounded();
    let mut names = vec![];
    app.new_window()
        .key_pressed(key_pressed)
        .size(700, 350)
        .view(view)
        .build()
        .unwrap();
    let audio_host = audio::Host::new();

    let (mut rack, mut controls, mut state, outputs, mut buffers) = tables();
    let mut oscs = vec![];
    let freq = 220.0;

    // Sine
    let sine = OscBuilder::new(sine_osc)
        .hz(freq)
        .rack(&mut rack, &mut controls, &mut state);
    oscs.push(sine.tag());
    names.push("Sine");

    // Square
    let square = OscBuilder::new(square_osc)
        .hz(freq)
        .rack(&mut rack, &mut controls, &mut state);
    oscs.push(square.tag());
    names.push("Square");

    // Saw
    let saw = OscBuilder::new(saw_osc)
        .hz(freq)
        .rack(&mut rack, &mut controls, &mut state);
    oscs.push(saw.tag());
    names.push("Saw");

    // Triangle
    let tri = OscBuilder::new(triangle_osc)
        .hz(freq)
        .rack(&mut rack, &mut controls, &mut state);
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
    let mix = MixerBuilder::new(vec![sine.tag(), square.tag()]).rack(&mut rack, &mut controls);
    oscs.push(mix.tag());
    names.push("Mixer Sine & Square");

    // Product
    let prod = ProductBuilder::new(vec![sine.tag(), pn.tag()]).rack(&mut rack, &mut controls);
    oscs.push(prod.tag());
    names.push("Product Sine & Square");

    // LFO
    let lfo = OscBuilder::new(sine_osc)
        .hz(2.0)
        .rack(&mut rack, &mut controls, &mut state);

    // Vca
    let vca = VcaBuilder::new(sine.tag())
        .level(lfo.tag())
        .rack(&mut rack, &mut controls);
    oscs.push(vca.tag());
    names.push("Vca amp controlled by sine");

    // CrossFade
    let cf = CrossFadeBuilder::new(sine.tag(), square.tag()).rack(&mut rack, &mut controls);
    cf.set_alpha(&mut controls, Control::V(lfo.tag(), 0));
    oscs.push(cf.tag());
    names.push("CrossFade Sine & Square, alpha is sine lfo");

    // Adsr
    let adsr = AdsrBuilder::linear()
        .attack(0.5)
        .decay(0.5)
        .sustain(0.75)
        .release(1.0)
        .rack(&mut rack, &mut controls);
    let adsr_vca = VcaBuilder::new(sine.tag())
        .level(adsr.tag())
        .rack(&mut rack, &mut controls);
    oscs.push(adsr_vca.tag());
    names.push("Adsr - . = on , = off");

    // FM
    let modulator = ModulatorBuilder::new(sine_osc)
        .hz(220.0)
        .ratio(2.0)
        .index(4.0)
        .rack(&mut rack, &mut controls, &mut state);
    let fm = OscBuilder::new(triangle_osc).hz(modulator.tag()).rack(
        &mut rack,
        &mut controls,
        &mut state,
    );
    oscs.push(fm.tag());
    names.push("FM synthesis");

    // LPF
    let lpf = LpfBuilder::new(square.tag())
        .cut_off(440.0)
        .rack(&mut rack, &mut controls);
    oscs.push(lpf.tag());
    names.push("Low Pass Filter");

    // Delay
    let delay =
        DelayBuilder::new(sine.tag(), 0.02.into()).rack(&mut rack, &mut controls, &mut buffers);
    let d = CrossFadeBuilder::new(sine.tag(), delay.tag()).rack(&mut rack, &mut controls);
    oscs.push(d.tag());
    names.push("Delay Line");

    let union = UnionBuilder::new(oscs).rack(&mut rack, &mut controls);
    let _out = VcaBuilder::new(union.tag())
        .level(0.25)
        .rack(&mut rack, &mut controls);

    let synth = Synth {
        sender,
        rack,
        controls,
        state,
        outputs,
        buffers,
        union,
        adsr,
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
    let sample_rate = buffer.sample_rate() as f32;
    for frame in buffer.frames_mut() {
        let amp = synth.rack.mono(
            &synth.controls,
            &mut synth.state,
            &mut synth.outputs,
            &mut synth.buffers,
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
                    let n = synth.names.len();
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
        Key::Period => {
            model
                .stream
                .send(|synth| {
                    synth.adsr.on(&mut synth.controls, &mut synth.state);
                })
                .unwrap();
        }
        Key::Comma => {
            model
                .stream
                .send(|synth| {
                    synth.adsr.off(&mut synth.controls);
                })
                .unwrap();
        }
        // Raise the frequency when the up key is pressed.
        _ => {}
    }
}
