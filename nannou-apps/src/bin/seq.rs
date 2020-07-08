use core::cmp::Ordering;
use crossbeam::crossbeam_channel::{unbounded, Receiver, Sender};
use nannou::{prelude::*, ui::prelude::*};
use nannou_audio as audio;
use nannou_audio::Buffer;
use pitch_calc::Letter;
use swell::filters::Lpf;
use swell::operators::Product;
use swell::oscillators::*;
use swell::sequencer::{Sequencer, Note, GateSeq, PitchSeq};
use swell::signal::{arc, Builder, Rack, Real, Signal};
use swell::reverb::Freeverb;

fn main() {
    nannou::app(model).update(update).run();
}

#[allow(dead_code)]
struct Model {
    ui: Ui,
    stream: audio::Stream<Synth>,
    receiver: Receiver<f32>,
    amps: Vec<f32>,
    max_amp: f32,
}

struct Synth {
    rack: Rack,
    sender: Sender<f32>,
}

fn build_synth(sender: Sender<f32>) -> Synth {
    let mut rack = Rack::new(vec![]);

    let notes = vec![
        Note::new(Letter::D, 1, true),
        Note::new(Letter::A, 2, true),
        Note::new(Letter::D, 2, true),
        Note::new(Letter::F, 2, true),
        Note::new(Letter::A, 3, true),
        Note::new(Letter::D, 3, true),
        Note::new(Letter::F, 3, true),
        Note::new(Letter::Csh, 2, true),
        Note::new(Letter::Csh, 3, true),

    ];
    let seq = Sequencer::new().sequence(notes).bpm(120.0).build();
    let mut pitch_seq = PitchSeq::new(seq.clone());
    rack.append(pitch_seq.wrap());

    let mut gate_seq = GateSeq::new(seq);
    rack.append(gate_seq.wrap());

    let wave = SawOsc::new().hz(pitch_seq.tag()).wrap();
    rack.append(wave.clone());

    let lpf = Lpf::new(wave.tag()).cutoff_freq(400).wrap();
    rack.append(lpf.clone());

    let reverb = arc(Freeverb::new(lpf.tag()));
    rack.append(reverb.clone());

    let prod = Product::new(vec![reverb.clone().tag(), gate_seq.tag()]).wrap();
    rack.append(prod);

    Synth { rack, sender }
}

fn model(app: &App) -> Model {
    let (sender, receiver) = unbounded();

    let _window = app.new_window().size(900, 520).view(view).build().unwrap();

    let ui = app.new_ui().build().unwrap();

    let audio_host = audio::Host::new();
    let synth = build_synth(sender);
    let stream = audio_host
        .new_output_stream(synth)
        .render(audio)
        .build()
        .unwrap();

    Model {
        ui,
        stream,
        receiver,
        amps: vec![],
        max_amp: 0.,
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
    let clone = amps.clone();

    // find max amplitude in waveform
    let max = amps.iter().max_by(|x, y| {
        if x > y {
            Ordering::Greater
        } else {
            Ordering::Less
        }
    });

    // store if it's greater than the previously stored max
    if max.is_some() && *max.unwrap() > model.max_amp {
        model.max_amp = *max.unwrap();
    }

    model.amps = clone;
}

fn view(app: &App, model: &Model, frame: Frame) {
    use nannou_apps::scope;
    scope(app, &model.amps, frame);
}
