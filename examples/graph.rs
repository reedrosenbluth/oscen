use core::cmp::Ordering;
use core::time::Duration;
use crossbeam::crossbeam_channel::{unbounded, Receiver, Sender};
use midir::{Ignore, MidiInput};
use nannou::prelude::*;
use nannou_audio as audio;
use nannou_audio::Buffer;
use pitch_calc::calc::hz_from_step;
use std::error::Error;
use std::{
    io::{stdin, stdout, Write},
    thread,
};
use swell::graph::*;
use swell::oscillators::*;
use swell::envelopes::*;
use swell::operators::*;

fn main() {
    nannou::app(model).update(update).run();
}

struct Synth {
    voice: Graph,
    sender: Sender<f32>,
}

struct Model {
    stream: audio::Stream<Synth>,
    receiver: Receiver<f32>,
    midi_receiver: Receiver<Vec<u8>>,
    amps: Vec<f32>,
    max_amp: f32,
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

    let _window = app.new_window().size(900, 620).view(view).build().unwrap();

    let audio_host = audio::Host::new();

    // let squarewave = SquareOsc::new(fix(220.0));
    // let osc01 = Osc01::new(fix(1.0));
    // let mut lerp = Lerp::new(0, 1);
    // lerp.alpha = In::Var(2);

    let sinewave = SineOsc::new(fix(10.0));
    let mut modu = Modulator::new(0, 220.0, 110.0);
    modu.mod_idx = fix(8.0);
    let fm = SineOsc::new(var(1));
    let sustain = SustainSynth::new(2);

    let voice = Graph::new(vec![arc(sinewave), arc(modu), arc(fm), arc(sustain)]);
    let synth = Synth { voice, sender };
    let stream = audio_host
        .new_output_stream(synth)
        .render(audio)
        .build()
        .unwrap();

    Model {
        stream,
        receiver,
        midi_receiver,
        amps: vec![],
        max_amp: 0.,
    }
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
            midi_sender.send(message.to_vec()).unwrap();
        },
        (),
    )?;

    input.clear();
    stdin().read_line(&mut input)?; // wait for next enter key press

    println!("Closing connection");
    Ok(())
}

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
                        if let Some(v) = synth.voice.0[0]
                            .module
                            .lock()
                            .unwrap()
                            .as_any_mut()
                            .downcast_mut::<SineOsc>()
                        {
                            v.hz = fix(hz);
                        }
                        if let Some(v) = synth.voice.0[1]
                            .module
                            .lock()
                            .unwrap()
                            .as_any_mut()
                            .downcast_mut::<SquareOsc>()
                        {
                            v.hz = fix(hz);
                        }
                        if let Some(v) = synth.voice.0[3]
                            .module
                            .lock()
                            .unwrap()
                            .as_any_mut()
                            .downcast_mut::<SustainSynth>()
                        {
                            v.on();
                        }
                    })
                    .unwrap();
            } else if message[0] == 128 {
                model
                    .stream
                    .send(move |synth| {
                        let step = message[1];
                        let hz = hz_from_step(step as f32) as Real;
                        if let Some(v) = synth.voice.0[0]
                            .module
                            .lock()
                            .unwrap()
                            .as_any_mut()
                            .downcast_mut::<SineOsc>()
                        {
                            v.hz = fix(hz);
                        }
                        if let Some(v) = synth.voice.0[1]
                            .module
                            .lock()
                            .unwrap()
                            .as_any_mut()
                            .downcast_mut::<SquareOsc>()
                        {
                            v.hz = fix(hz);
                        }
                        if let Some(v) = synth.voice.0[3]
                            .module
                            .lock()
                            .unwrap()
                            .as_any_mut()
                            .downcast_mut::<SustainSynth>()
                        {
                            v.off();
                        }
                        // synth.voice.on();
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
            .x_y(-300., 0.);

        draw.to_frame(app, &frame).unwrap();
    }
}
