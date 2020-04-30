use core::cmp::Ordering;
use core::time::Duration;
use std::thread;
use std::error::Error;
use std::io::{stdin, stdout, Write};
use crossbeam::crossbeam_channel::{unbounded, Receiver, Sender};
use nannou::prelude::*;
use nannou::ui::prelude::*;
use nannou_audio as audio;
use nannou_audio::Buffer;
use midir::{MidiInput, Ignore};
use pitch_calc::calc::{hz_from_step};

use swell::dsp::*;
use swell::shaper::*;
use swell::containers::*;

fn main() {
    nannou::app(model).update(update).run();
}

struct Model {
    ui: Ui,
    ids: Ids,
    pitch: Hz,
    shape: Amp,
    attack: Amp,
    decay: Amp,
    sustain_time: Amp,
    sustain_level: Amp,
    release: Amp,
    stream: audio::Stream<Synth>,
    receiver: Receiver<f32>,
    midi_receiver: Receiver<Vec<u8>>,
    amps: Vec<f32>,
    max_amp: f32,
}

struct Ids {
    pitch: widget::Id,
    shape: widget::Id,
    attack: widget::Id,
    decay: widget::Id,
    sustain_time: widget::Id,
    sustain_level: widget::Id,
    release: widget::Id,
}

struct Synth {
    voice: TriggerSynth<WaveShaper>,
    sender: Sender<f32>,
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
            println!("Choosing the only available input port: {}", midi_in.port_name(0).unwrap());
            0
        },
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
    let _conn_in = midi_in.connect(in_port, "midir-read-input", move |stamp, message, _| {
        println!("{}: {:?} (len = {})", stamp, message, message.len());
        midi_sender.send(message.to_vec()).unwrap();
    }, ())?;
    
    // println!("Connection open, reading input from '{}' (press enter to exit) ...", in_port_name);

    input.clear();
    stdin().read_line(&mut input)?; // wait for next enter key press

    println!("Closing connection");
    Ok(())
}

fn model(app: &App) -> Model {
    let (sender, receiver) = unbounded();
    let (midi_sender, midi_receiver) = unbounded();

    thread::spawn(|| {
        match listen_midi(midi_sender) {
            Ok(_) => (),
            Err(err) => println!("Error: {}", err)
        }
    });

    // Create a window to receive key pressed events.
    app.set_loop_mode(LoopMode::Rate {
        update_interval: Duration::from_millis(1),
    });

    let _window = app
        .new_window()
        .size(900, 620)
        .key_pressed(key_pressed)
        .view(view)
        .build()
        .unwrap();

    let mut ui = app.new_ui().build().unwrap();

    let ids = Ids {
        pitch: ui.generate_widget_id(),
        shape: ui.generate_widget_id(),
        attack: ui.generate_widget_id(),
        decay: ui.generate_widget_id(),
        sustain_time: ui.generate_widget_id(),
        sustain_level: ui.generate_widget_id(),
        release: ui.generate_widget_id(),
    };
    let audio_host = audio::Host::new();

    let wave_shaper = WaveShaper::wrapped(440., 0.5);
    let voice = TriggerSynth::new(wave_shaper, 0.2, 0.1, 5.0, 0.8, 0.2);
    let synth = Synth { voice, sender };
    let stream = audio_host
        .new_output_stream(synth)
        .render(audio)
        .build()
        .unwrap();

    Model {
        ui,
        ids,
        pitch: 440.,
        shape: 0.5,
        attack: 0.2,
        decay: 0.1,
        sustain_time: 5.0,
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
    match key {
        Key::Space => {
            model
                .stream
                .send(move |synth| {
                    synth.voice.on();
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
                        let hz = hz_from_step(step as f32);
                        synth.voice.set_hz(hz as f64);
                        synth.voice.on();
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

    fn slider(val: f32, min: f32, max: f32) -> widget::Slider<'static, f32> {
        widget::Slider::new(val, min, max)
            .w_h(200.0, 30.0)
            .label_font_size(15)
            .rgb(0.1, 0.2, 0.5)
            .label_rgb(1.0, 1.0, 1.0)
            .border(0.0)
    }

    for value in slider(model.shape as f32, 0., 1.)
        .top_left_with_margin(20.0)
        .label("Shape (PW <-> Sin <-> Saw)")
        .set(model.ids.shape, ui)
    {
        model.shape = value;
        model
            .stream
            .send(move |synth| {
              synth.voice.wave.mtx().knob = value;
              synth.voice.wave.mtx().set_alphas();
            })
            .unwrap();
    }

    for value in slider(model.pitch as f32, 0., 880.)
        .down(20.0)
        .label("Pitch")
        .set(model.ids.pitch, ui)
    {
        model.pitch = value as Hz;
        model
            .stream
            .send(move |synth| synth.voice.set_hz(value as Hz))
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
                synth.voice.attack = value;
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
                synth.voice.decay = value;
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
                synth.voice.sustain_time = value;
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
                synth.voice.sustain_level = value;
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
                synth.voice.release = value;
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
