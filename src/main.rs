use core::cmp::Ordering;
use core::time::Duration;
use crossbeam::crossbeam_channel::{unbounded, Receiver, Sender};
use nannou::prelude::*;
use nannou::ui::prelude::*;
use nannou_audio as audio;
use nannou_audio::Buffer;

use swell::dsp::*;
use swell::shaper::*;

fn main() {
    // nannou::app(model).update(update).simple_window(view).run();
    nannou::app(model).update(update).run();
}

struct Model {
    ui: Ui,
    ids: Ids,
    knob: Amp,
    ratio: Hz,
    carrier_hz: Hz,
    mod_idx: Phase,
    attack: Amp,
    decay: Amp,
    sustain_time: Amp,
    sustain_level: Amp,
    release: Amp,
    stream: audio::Stream<Synth>,
    receiver: Receiver<f32>,
    amps: Vec<f32>,
    max_amp: f32,
}

struct Ids {
    knob: widget::Id,
    ratio: widget::Id,
    carrier_hz: widget::Id,
    mod_idx: widget::Id,
    attack: widget::Id,
    decay: widget::Id,
    sustain_time: widget::Id,
    sustain_level: widget::Id,
    release: widget::Id,
}

struct Synth {
    // voice: Box<dyn Wave + Send>,
    voice: ShaperSynth,
    sender: Sender<f32>,
}

fn model(app: &App) -> Model {
    let (sender, receiver) = unbounded();

    // Create a window to receive key pressed events.
    app.set_loop_mode(LoopMode::Rate {
        update_interval: Duration::from_millis(1),
    });

    let _window = app
        .new_window().size(900, 600)
        .key_pressed(key_pressed)
        .view(view)
        .build()
        .unwrap();

    let mut ui = app.new_ui().build().unwrap();

    let ids = Ids {
        knob: ui.generate_widget_id(),
        ratio: ui.generate_widget_id(),
        carrier_hz: ui.generate_widget_id(),
        mod_idx: ui.generate_widget_id(),
        attack: ui.generate_widget_id(),
        decay: ui.generate_widget_id(),
        sustain_time: ui.generate_widget_id(),
        sustain_level: ui.generate_widget_id(),
        release: ui.generate_widget_id(),
    };
    let audio_host = audio::Host::new();

    let mut voice = ShaperSynth::new(440., 8.0, 1.0, 0.2, 0.1, 5.0, 0.85, 0.2, 400., 0.707);
    voice.0.off = true;
    let synth = Synth { voice, sender };
    let stream = audio_host
        .new_output_stream(synth)
        .render(audio)
        .build()
        .unwrap();

    Model {
        ui,
        ids,
        knob: 0.5,
        ratio: 8.0,
        carrier_hz: 440.,
        mod_idx: 1.0,
        attack: 0.2,
        decay: 0.1,
        sustain_time: 5.0,
        sustain_level: 0.8,
        release: 0.2,
        stream,
        receiver,
        amps: vec![],
        max_amp: 0.,
    }
}

// A function that renders the given `Audio` to the given `Buffer`.
// In this case we play a simple sine wave at the audio's current frequency in `hz`.
fn audio(synth: &mut Synth, buffer: &mut Buffer) {
    let sample_rate = buffer.sample_rate() as f64;
    for frame in buffer.frames_mut() {
        let mut amp = 0.;
        amp += synth.voice.signal(sample_rate);
        for channel in frame {
            *channel = amp;
        }
        synth.sender.send(amp).unwrap();
    }
}

fn key_pressed(_app: &App, model: &mut Model, key: Key) {
    model.max_amp = 0.;
    let change_hz = |i| {
        model
            .stream
            .send(move |synth| {
                let factor = 2.0.powf(i / 12.);
                // synth.voice.carrier.lock().unwrap().hz *= factor;
            })
            .unwrap();
    };
    match key {
        // Pause or unpause the audio when Space is pressed.
        Key::Space => {
            model
                .stream
                .send(move |synth| {
                    synth.voice.0.wave.lock().unwrap().on();
                })
                .unwrap();
        }
        // Raise the frequency when the up key is pressed.
        Key::Up => change_hz(1.),
        Key::Down => change_hz(-1.),
        _ => {}
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

    //UI
    let ui = &mut model.ui.set_widgets();

    fn slider(val: f32, min: f32, max: f32) -> widget::Slider<'static, f32> {
        widget::Slider::new(val, min, max)
            .w_h(200.0, 30.0)
            .label_font_size(15)
            .rgb(0.1, 0.2, 0.5)
            .label_rgb(1.0, 1.0, 1.0)
            .border(0.0)
    }

    for value in slider(model.knob as f32, 0., 1.)
        .top_left_with_margin(20.0)
        .label("Wave Knob")
        .set(model.ids.knob, ui)
    {
        model.knob = value;
        model
            .stream
            .send(move |synth| synth.voice.set_knob(value))
            .unwrap();
    }

    for value in slider(model.ratio as f32, 0.5, 16.)
        .down(20.)
        .label("Ratio")
        .set(model.ids.ratio, ui)
    {
        let value = value as f64;
        model.ratio = value;
        model
            .stream
            .send(move |synth| {
                synth.voice.set_ratio(value);
            })
            .unwrap();
    }

    for value in slider(model.carrier_hz as f32, 55., 3200.)
        .down(20.)
        .label("Carrier hz")
        .set(model.ids.carrier_hz, ui)
    {
        let value = value as f64;
        model.carrier_hz = value;
        model
            .stream
            .send(move |synth| {
                synth.voice.set_carrier_hz(value);
            })
            .unwrap();
    }

    for value in slider(model.mod_idx as f32, 0.0, 16.)
        .down(20.)
        .label("Modulation Index")
        .set(model.ids.mod_idx, ui)
    {
        let value = value as f64;
        model.mod_idx = value;
        model
            .stream
            .send(move |synth| {
                synth.voice.set_mod_idx(value);
            })
            .unwrap();
    }

    for value in slider(model.attack, 0.0, 1.0)
        .down(20.)
        .label("Attack")
        .set(model.ids.attack, ui)
    {
        model.attack = value;
        model
            .stream
            .send(move |synth| {
                synth.voice.set_attack(value);
            })
            .unwrap();
    }

    for value in slider(model.decay, 0.0, 1.0)
        .down(20.)
        .label("Decay")
        .set(model.ids.decay, ui)
    {
        model.decay = value;
        model
            .stream
            .send(move |synth| {
                synth.voice.set_decay(value);
            })
            .unwrap();
    }

    for value in slider(model.sustain_time, 0.0, 10.0)
        .down(20.)
        .label("Sustain Time")
        .set(model.ids.sustain_time, ui)
    {
        model.sustain_time = value;
        model
            .stream
            .send(move |synth| {
                synth.voice.set_sustain_time(value);
            })
            .unwrap();
    }

    for value in slider(model.sustain_level, 0.0, 1.0)
        .down(20.)
        .label("Sustain Level")
        .set(model.ids.sustain_level, ui)
    {
        model.sustain_level = value;
        model
            .stream
            .send(move |synth| {
                synth.voice.set_sustain_level(value);
            })
            .unwrap();
    }

    for value in slider(model.release, 0.0, 1.0)
        .down(20.)
        .label("Release")
        .set(model.ids.release, ui)
    {
        model.release = value;
        model
            .stream
            .send(move |synth| {
                synth.voice.set_release(value);
            })
            .unwrap();
    }
}

fn view(app: &App, model: &Model, frame: Frame) {
    let draw = app.draw();
    let c = rgb(9. / 255., 9. / 255., 44. / 255.);
    draw.background().color(c);
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
