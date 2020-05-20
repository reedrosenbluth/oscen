use core::cmp::Ordering;
use core::time::Duration;
use crossbeam::crossbeam_channel::{unbounded, Receiver, Sender};
use midir::{Ignore, MidiInput};
use nannou::prelude::*;
use nannou::ui::prelude::*;
use nannou_audio as audio;
use nannou_audio::Buffer;
use pitch_calc::calc::hz_from_step;
use std::error::Error;
use std::io::{stdin, stdout, Write};
use std::thread;
use swell::envelopes::*;
use swell::filters::*;
use swell::graph::*;
use swell::operators::*;
use swell::oscillators::*;

fn main() {
    nannou::app(model).update(update).run();
}

struct Model {
    ui: Ui,
    ids: Ids,
    knob: Real,
    ratio: Real,
    mod_idx: Real,
    cutoff: Real,
    q: Real,
    t: Real,
    attack: Real,
    decay: Real,
    sustain_level: Real,
    release: Real,
    stream: audio::Stream<Synth>,
    receiver: Receiver<f32>,
    midi_receiver: Receiver<Vec<u8>>,
    amps: Vec<f32>,
    max_amp: f32,
}

struct Ids {
    knob: widget::Id,
    ratio: widget::Id,
    mod_idx: widget::Id,
    cutoff: widget::Id,
    q: widget::Id,
    t: widget::Id,
    attack: widget::Id,
    decay: widget::Id,
    sustain_level: widget::Id,
    release: widget::Id,
}

fn listen_midi(midi_sender: Sender<Vec<u8>>) -> Result<(), Box<dyn Error>> {
    let mut input = String::new();
    let mut midi_in = MidiInput::new("midir reading input")?;
    midi_in.ignore(Ignore::None);

    // Get an input port (read from console if multiple are available)
    let in_ports = midi_in.port_count();
    let in_port = match in_ports {
        0 => return Err("no input port found".into()),
        1 => {
            println!(
                "Choosing the only available input port: {}",
                midi_in.port_name(0).unwrap()
            );
            0
        }
        _ => {
            println!("\nAvailable input ports:");
            for i in 0..in_ports {
                println!("{}: {}", i, midi_in.port_name(i).unwrap());
            }
            print!("Please select input port: ");
            stdout().flush()?;
            let mut input = String::new();
            stdin().read_line(&mut input)?;
            input.trim().parse::<usize>()?
        }
    };

    println!("\nOpening connection");

    // _conn_in needs to be a named parameter, because it needs to be kept alive until the end of the scope
    let _conn_in = midi_in.connect(
        in_port,
        "midir-read-input",
        move |_, message, _| {
            // println!("{}: {:?} (len = {})", stamp, message, message.len());
            midi_sender.send(message.to_vec()).unwrap();
        },
        (),
    )?;

    // println!("Connection open, reading input from '{}' (press enter to exit) ...", in_port_name);

    input.clear();
    stdin().read_line(&mut input)?; // wait for next enter key press

    println!("Closing connection");
    Ok(())
}

struct Synth {
    // voice: Box<dyn Wave + Send>,
    voice: Graph,
    sender: Sender<f32>,
}

fn model(app: &App) -> Model {
    let (sender, receiver) = unbounded();
    let (midi_sender, midi_receiver) = unbounded();

    thread::spawn(|| match listen_midi(midi_sender) {
        Ok(_) => (),
        Err(err) => println!("Error: {}", err),
    });

    // Create a window to receive key pressed events.
    app.set_loop_mode(LoopMode::Rate {
        update_interval: Duration::from_millis(1),
    });

    let _window = app
        .new_window()
        .size(900, 520)
        .key_pressed(key_pressed)
        .view(view)
        .build()
        .unwrap();

    let mut ui = app.new_ui().build().unwrap();

    let ids = Ids {
        knob: ui.generate_widget_id(),
        ratio: ui.generate_widget_id(),
        mod_idx: ui.generate_widget_id(),
        cutoff: ui.generate_widget_id(),
        q: ui.generate_widget_id(),
        t: ui.generate_widget_id(),
        attack: ui.generate_widget_id(),
        decay: ui.generate_widget_id(),
        sustain_level: ui.generate_widget_id(),
        release: ui.generate_widget_id(),
    };
    let audio_host = audio::Host::new();

    let node_0 = SineOsc::wrapped(fix(440.0));
    let node_1 = Modulator::wrapped(0, 110., 10.);
    let node_2 = SquareOsc::wrapped(fix(440.));
    let node_3 = SineOsc::wrapped(fix(440.));
    let node_4 = SawOsc::wrapped(fix(440.));
    let node_5 = Lerp::wrapped(2, 3);
    let node_6 = Lerp::wrapped(3, 4);
    let node_7 = Lerp3::wrapped(5, 6, fix(0.5));
    let node_8 = BiquadFilter::lphpf(7, 44100.0, 440., 0.707, 1.0);
    let node_9 = SustainSynth::wrapped(8);
    let voice = Graph::new(vec![
        node_0,
        node_1,
        node_2,
        node_3,
        node_4,
        node_5,
        node_6,
        node_7,
        arc(node_8),
        node_9,
    ]);

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
        ratio: 1.0,
        mod_idx: 0.0,
        cutoff: 0.0,
        q: 0.707,
        t: 1.0,
        attack: 0.2,
        decay: 0.1,
        sustain_level: 0.8,
        release: 0.2,
        stream,
        receiver,
        midi_receiver,
        amps: vec![],
        max_amp: 0.,
    }
}

// A function that renders the given `Audio` to the given `Buffer`.
// In this case we play a simple sine wave at the audio's current frequency in `hz`.
fn audio(synth: &mut Synth, buffer: &mut Buffer) {
    let sample_rate = buffer.sample_rate() as Real;
    for frame in buffer.frames_mut() {
        let mut amp = 0.;
        amp += synth.voice.signal(sample_rate);
        for channel in frame {
            *channel = amp as f32;
        }
        synth.sender.send(amp as f32).unwrap();
    }
}

fn key_pressed(_app: &App, model: &mut Model, key: Key) {
    model.max_amp = 0.;
    match key {
        // Pause or unpause the audio when Space is pressed.
        Key::Space => {
            model
                .stream
                .send(move |synth| {
                    // synth.voice.0.lphp.wave.mtx().on();
                })
                .unwrap();
        }
        _ => {}
    }
}

fn update(_app: &App, model: &mut Model, _update: Update) {
    let midi_messages: Vec<Vec<u8>> = model.midi_receiver.try_iter().collect();
    for message in midi_messages {
        if message.len() == 3 {
            if message[0] == 144 {
                model
                    .stream
                    .send(move |synth| {
                        let step = message[1];
                        let hz = hz_from_step(step as f32) as Real;
                        if let Some(v) = synth.voice.nodes[2]
                            .module
                            .lock()
                            .unwrap()
                            .as_any_mut()
                            .downcast_mut::<SquareOsc>()
                        {
                            v.hz = fix(hz);
                        }
                        if let Some(v) = synth.voice.nodes[3]
                            .module
                            .lock()
                            .unwrap()
                            .as_any_mut()
                            .downcast_mut::<SineOsc>()
                        {
                            v.hz = fix(hz);
                        }
                        if let Some(v) = synth.voice.nodes[4]
                            .module
                            .lock()
                            .unwrap()
                            .as_any_mut()
                            .downcast_mut::<SawOsc>()
                        {
                            v.hz = fix(hz);
                        }
                        if let Some(v) = synth.voice.nodes[9]
                            .module
                            .lock()
                            .unwrap()
                            .as_any_mut()
                            .downcast_mut::<SustainSynth>()
                        {
                            v.triggered = true;
                        }
                    })
                    .unwrap();
            } else if message[0] == 128 {
                model
                    .stream
                    .send(move |synth| {
                        if let Some(v) = synth.voice.nodes[9]
                            .module
                            .lock()
                            .unwrap()
                            .as_any_mut()
                            .downcast_mut::<SustainSynth>()
                        {
                            v.triggered = false;
                        }
                    })
                    .unwrap();
            }
        }
    }

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

    fn slider<T>(val: T, min: T, max: T) -> widget::Slider<'static, T>
    where
        T: Float,
    {
        widget::Slider::new(val, min, max)
            .w_h(200.0, 30.0)
            .label_font_size(15)
            .rgb(0.1, 0.2, 0.5)
            .label_rgb(1.0, 1.0, 1.0)
            .border(0.0)
    }

    for value in slider(model.knob, 0., 1.)
        .top_left_with_margin(20.0)
        .label(format!("Wave Knob: {:.2}", model.knob).as_str())
        .set(model.ids.knob, ui)
    {
        model.knob = value;
        model
            .stream
            .send(move |synth| {
                if let Some(v) = synth.voice.nodes[7]
                    .module
                    .lock()
                    .unwrap()
                    .as_any_mut()
                    .downcast_mut::<Lerp3>()
                {
                    v.knob = fix(value);
                }
            })
            .unwrap();
    }

    for value in slider(model.ratio, 1.0, 16.)
        .down(20.)
        .label(format!("Ratio: {:.2}", model.ratio).as_str())
        .set(model.ids.ratio, ui)
    {
        let value = math::round::half_up(value, 0);
        model.ratio = value;
        model
            .stream
            .send(move |synth| {
                // synth.voice.set_ratio(value);
            })
            .unwrap();
    }

    for value in slider(model.mod_idx, 0.0, 16.)
        .down(20.)
        .label(format!("Modulation Index: {:.2}", model.mod_idx).as_str())
        .set(model.ids.mod_idx, ui)
    {
        model.mod_idx = value;
        model
            .stream
            .send(move |synth| {
                // synth.voice.set_mod_idx(value);
            })
            .unwrap();
    }

    for value in slider(model.cutoff, 0.0, 2400.0)
        .down(20.)
        .label(format!("Filter Cutoff: {:.1}", model.cutoff).as_str())
        .set(model.ids.cutoff, ui)
    {
        model.cutoff = value;
        model
            .stream
            .send(move |synth| {
                if value < 1.0 {
                    if let Some(v) = synth.voice.nodes[9]
                        .module
                        .lock()
                        .unwrap()
                        .as_any_mut()
                        .downcast_mut::<BiquadFilter>()
                    {
                        v.off = true;
                    }
                } else {
                    if let Some(v) = synth.voice.nodes[9]
                        .module
                        .lock()
                        .unwrap()
                        .as_any_mut()
                        .downcast_mut::<BiquadFilter>()
                    {
                        v.off = false;
                        // v.cutoff = fix(value);
                    }
                    // synth.voice.0.lphp.off = fals;
                    // synth.voice.set_cutoff(value);
                }
            })
            .unwrap();
    }

    for value in slider(model.q, 0.7071, 10.0)
        .down(20.)
        .label(format!("Filter Q: {:.3}", model.q).as_str())
        .set(model.ids.q, ui)
    {
        model.q = value;
        model
            .stream
            .send(move |synth| {
                // synth.voice.set_q(value);
            })
            .unwrap();
    }

    for value in slider(model.t as f32, 0.0, 1.0)
        .down(20.)
        .label(format!("Filter Knob: {:.2}", model.t).as_str())
        .set(model.ids.t, ui)
    {
        let value = value as Real;
        model.t = value;
        model
            .stream
            .send(move |synth| {
                // synth.voice.set_t(value);
            })
            .unwrap();
    }

    for value in slider(model.attack, 0.0, 1.0)
        .down(20.)
        .label(format!("Attack: {:.2}", model.attack).as_str())
        .set(model.ids.attack, ui)
    {
        model.attack = value;
        model
            .stream
            .send(move |synth| {
                // synth.voice.set_attack(value);
            })
            .unwrap();
    }

    for value in slider(model.decay, 0.0, 1.0)
        .down(20.)
        .label(format!("Decay: {:.2}", model.decay).as_str())
        .set(model.ids.decay, ui)
    {
        model.decay = value;
        model
            .stream
            .send(move |synth| {
                // synth.voice.set_decay(value);
            })
            .unwrap();
    }

    for value in slider(model.sustain_level, 0.05, 1.0)
        .down(20.)
        .label(format!("Sustain Level: {:.2}", model.sustain_level).as_str())
        .set(model.ids.sustain_level, ui)
    {
        model.sustain_level = value;
        model
            .stream
            .send(move |synth| {
                // synth.voice.set_sustain_level(value);
            })
            .unwrap();
    }

    for value in slider(model.release, 0.0, 1.0)
        .down(20.)
        .label(format!("Release: {:.2}", model.release).as_str())
        .set(model.ids.release, ui)
    {
        model.release = value;
        model
            .stream
            .send(move |synth| {
                // synth.voice.set_release(value);
            })
            .unwrap();
    }
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
