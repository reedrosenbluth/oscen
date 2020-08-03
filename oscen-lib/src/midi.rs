use super::signal::*;
use super::utils::ExpInterp;
use crate::{as_any_mut, std_signal};
use crossbeam::crossbeam_channel::Sender;
use midir::{Ignore, MidiInput};
use pitch_calc::calc::hz_from_step;
use std::any::Any;
use std::error::Error;
use std::io::{stdin, stdout, Write};

/// The most recent note received from the midi source.
#[derive(Clone)]
pub struct MidiPitch {
    tag: Tag,
    step: f32,
    offset: f32, // In semitones
    factor: f32,
}

impl MidiPitch {
    pub fn new() -> Self {
        MidiPitch {
            tag: 0,
            step: 0.0,
            offset: 0.0,
            factor: 1.0,
        }
    }

    pub fn step(&mut self, arg: f32) -> &mut Self {
        self.step = arg;
        self
    }

    pub fn offset(&mut self, arg: f32) -> &mut Self {
        self.offset = arg;
        self
    }

    pub fn factor(&mut self, arg: f32) -> &mut Self {
        self.factor = arg;
        self
    }
}

impl Builder for MidiPitch {}

impl Signal for MidiPitch {
    std_signal!();
    fn signal(&mut self, _rack: &Rack, _sample_rate: Real) -> Real {
        hz_from_step(self.factor * self.step + self.offset) as Real
    }
}

#[derive(Clone)]
pub struct MidiControl {
    tag: Tag,
    pub controller: u8,
    value: u8,
    exp_interp: ExpInterp,
}

impl MidiControl {
    pub fn new(controller: u8, value: u8, low: Real, mid: Real, high: Real) -> Self {
        Self {
            tag: 0,
            controller,
            value,
            exp_interp: ExpInterp::new(low, mid, high),
        }
    }

    fn map_range(&self, input: Real) -> Real {
        let x = input / 127.0;
        self.exp_interp.interp(x)
    }

    pub fn controller(&mut self, arg: u8) -> &mut Self {
        self.controller = arg;
        self
    }

    pub fn value(&mut self, arg: u8) -> &mut Self {
        self.value = arg;
        self
    }
}

impl Builder for MidiControl {}

impl Signal for MidiControl {
    std_signal!();

    fn signal(&mut self, _rack: &Rack, _sample_rate: Real) -> Real {
        self.map_range(self.value as Real)
    }
}

pub fn listen_midi(midi_sender: Sender<Vec<u8>>) -> Result<(), Box<dyn Error>> {
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

    // println!("Connection open, reading input from '{}' (press enter to exit) ...", in_port_name);

    input.clear();
    stdin().read_line(&mut input)?; // wait for next enter key press

    println!("Closing connection");
    Ok(())
}
