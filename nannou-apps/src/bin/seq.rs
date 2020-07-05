use core::cmp::Ordering;
use core::time::Duration;
use crossbeam::crossbeam_channel::{unbounded, Receiver, Sender};
use nannou::{prelude::*, ui::prelude::*};
use nannou_audio as audio;
use nannou_audio::Buffer;
use pitch_calc::Letter;
use swell::filters::Lpf;
use swell::operators::Product;
use swell::oscillators::TriangleOsc;
use swell::sequencer::{Sequencer, Note, GateSeq, PitchSeq};
use swell::signal::{Builder, Rack, Real, Signal};

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
        Note::new(Letter::F, 2, true),
        Note::new(Letter::B, 2, true),
        Note::new(Letter::Dsh, 3, true),
        Note::new(Letter::Gsh, 3, true),
        Note::new(Letter::E, 2, true),
        Note::new(Letter::Gsh, 2, true),
        Note::new(Letter::D, 3, true),
        Note::new(Letter::Ash, 3, true),

    ];
    let seq = Sequencer::new().sequence(notes).bpm(320.0).build();
    let mut pitch_seq = PitchSeq::new(seq.clone());
    rack.append(pitch_seq.wrap());

    let mut gate_seq = GateSeq::new(seq);
    rack.append(gate_seq.wrap());

    let wave = TriangleOsc::new().hz(pitch_seq.tag()).wrap();
    rack.append(wave.clone());

    let lpf = Lpf::new(wave.tag()).cutoff_freq(440).wrap();
    rack.append(lpf.clone());

    let prod = Product::new(vec![lpf.tag(), gate_seq.tag()]).wrap();
    rack.append(prod);

    Synth { rack, sender }
}

fn model(app: &App) -> Model {
    let (sender, receiver) = unbounded();

    // Create a window to receive key pressed events.
    app.set_loop_mode(LoopMode::Rate {
        update_interval: Duration::from_millis(1),
    });

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
    let draw = app.draw();
    let c = rgb(9. / 255., 9. / 255., 44. / 255.);
    draw.background().color(c);
    if frame.nth() == 0 {
        draw.to_frame(app, &frame).unwrap()
    }
    let mut shifted: Vec<f32> = vec![];
    let mut iter = model.amps.iter().peekable();

    let mut i = 0;
    while iter.len() > 0 {
        let amp = iter.next().unwrap_or(&0.);
        if amp.abs() < 0.01 && **iter.peek().unwrap_or(&amp) > *amp {
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
        points.push(pt2(i as f32, amp * 120.));
    }

    // only draw if we got enough info back from the audio thread
    if points.len() == 600 {
        draw.path()
            .stroke()
            .weight(2.)
            .points(points)
            .color(CORNFLOWERBLUE)
            .x_y(-200., 0.);

        draw.to_frame(app, &frame).unwrap();
    }

    // Draw the state of the `Ui` to the frame.
    model.ui.draw_to_frame(app, &frame).unwrap();
}
