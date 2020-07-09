use crossbeam::crossbeam_channel::{unbounded, Receiver, Sender};
use nannou::{prelude::*, ui::prelude::*};
use nannou_audio as audio;
use nannou_audio::Buffer;
use std::thread;
use swell::instruments::WaveGuide;
use swell::midi::{listen_midi, MidiControl, MidiPitch};
use swell::oscillators::SquareOsc;
use swell::signal::{ArcMutex, Builder, Rack, Real, Signal, Tag, Gate};

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

#[derive(Clone)]
struct Midi {
    midi_pitch: ArcMutex<MidiPitch>,
}

struct Synth {
    midi: Midi,
    midi_receiver: Receiver<Vec<u8>>,
    rack: Rack,
    karplus_tag: Tag,
    sender: Sender<f32>,
}

fn build_synth(midi_receiver: Receiver<Vec<u8>>, sender: Sender<f32>) -> Synth {
    let mut rack = Rack::new(vec![]);

    //  Midi
    let midi_pitch = MidiPitch::new().rack(&mut rack);
    MidiControl::new(1, 64, 0.0, 0.5, 1.0).rack(&mut rack);

    let excite = SquareOsc::new().hz(110).rack(&mut rack);

    let karplus = WaveGuide::new(excite.tag())
        .hz(midi_pitch.tag())
        .wet_decay(0.95)
        .attack(0.005)
        .release(0.005)
        .rack(&mut rack);
    let karplus_tag = karplus.tag();

    Synth {
        midi: Midi { midi_pitch },
        midi_receiver,
        rack,
        karplus_tag,
        sender,
    }
}

fn model(app: &App) -> Model {
    let (sender, receiver) = unbounded();
    let (midi_sender, midi_receiver) = unbounded();

    thread::spawn(|| match listen_midi(midi_sender) {
        Ok(_) => (),
        Err(err) => println!("Error: {}", err),
    });

    let _window = app.new_window().size(900, 520).view(view).build().unwrap();

    let ui = app.new_ui().build().unwrap();

    let audio_host = audio::Host::new();
    let synth = build_synth(midi_receiver, sender);
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
    let midi_messages: Vec<Vec<u8>> = synth.midi_receiver.try_iter().collect();
    let karplus_tag = synth.karplus_tag;
    for message in midi_messages {
        if message.len() == 3 {
            let step = message[1] as f32;
            if message[0] == 144 {
                synth.midi.midi_pitch.lock().unwrap().step(step);
                WaveGuide::gate_on(&synth.rack, karplus_tag);
            } else if message[0] == 128 {
                WaveGuide::gate_off(&synth.rack, karplus_tag);
            }
        }
    }

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