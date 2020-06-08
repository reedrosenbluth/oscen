use super::signal::*;
use crate::{std_signal, as_any_mut};
use crossbeam::crossbeam_channel::Sender;
use midir::{Ignore, MidiInput};
use pitch_calc::calc::hz_from_step;
use std::any::Any;
use std::error::Error;
use std::io::{stdin, stdout, Write};

/// The most recent note received from the midi source.
#[derive(Clone)]
pub struct MidiPitch {
    pub tag: Tag,
    pub step: f32,
}

impl MidiPitch {
    pub fn new() -> Self {
        MidiPitch {
            tag: mk_tag(),
            step: 0.0,
        }
    }

    pub fn wrapped() -> ArcMutex<Self> {
        arc(Self::new())
    }

    pub fn set_step(&mut self, step: f32) {
        self.step = step;
    }
}

impl Signal for MidiPitch {
    std_signal!();
    fn signal(&mut self, _rack: &Rack, _sample_rate: Real) -> Real {
        hz_from_step(self.step) as Real
    }
}


#[derive(Clone)]
pub struct MidiControl {
    pub tag: Tag,
    pub controller: u8,
    pub value: u8,
    pub range: (Real, Real),
}

impl MidiControl {
    pub fn new(controller: u8) -> Self {
        Self {
            tag: mk_tag(),
            controller,
            value: 0,
            range: (0.0, 1.0),
        }
    }

    fn map_range(&self, input: Real) -> Real {
        let m = (self.range.1 - self.range.0) / 127.;
        self.range.0 + m * input
    }

    pub fn wrapped(controller: u8) -> ArcMutex<Self> {
        arc(Self::new(controller))
    }

    pub fn set_value(&mut self, value: u8) {
        self.value = value;
    }
}

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
