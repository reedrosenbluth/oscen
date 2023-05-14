use crate::rack::*;
use crate::utils::interp;
use crate::{build, props, tag};
use crossbeam::channel::Sender;
use midir::{Ignore, MidiInput};
use pitch_calc::calc::hz_from_step;
use std::error::Error;
use std::io::{stdin, stdout, Write};
use std::sync::Arc;

#[derive(Debug, Copy, Clone)]
pub struct MidiPitch {
    tag: Tag,
}

impl MidiPitch {
    pub fn new(tag: Tag) -> Self {
        Self { tag }
    }

    props!(step, set_step, 0);
    props!(offset, set_offset, 1);
    props!(factor, set_factor, 2);
}

impl Signal for MidiPitch {
    tag!();

    fn signal(&self, rack: &mut Rack, _sample_rate: f32) {
        rack.outputs[(self.tag, 0)] =
            hz_from_step(self.factor(rack) * self.step(rack) + self.offset(rack));
    }
}

#[derive(Debug, Copy, Clone)]
pub struct MidiPitchBuilder {
    step: Control,
    offset: Control,
    factor: Control,
}

impl Default for MidiPitchBuilder {
    fn default() -> Self {
        Self {
            step: 0.0.into(),
            offset: 0.0.into(),
            factor: 1.0.into(),
        }
    }
}

impl MidiPitchBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    build!(step);
    build!(offset);
    build!(factor);

    pub fn rack(&self, rack: &mut Rack) -> Arc<MidiPitch> {
        let n = rack.num_modules();
        rack.controls[(n, 0)] = self.step;
        rack.controls[(n, 1)] = self.offset;
        rack.controls[(n, 2)] = self.factor;
        let mp = Arc::new(MidiPitch::new(n.into()));
        rack.push(mp.clone());
        mp
    }
}

#[derive(Debug, Copy, Clone)]
pub struct MidiControl {
    tag: Tag,
    controller: u8,
    low: f32,
    mid: f32,
    high: f32,
}

impl MidiControl {
    pub fn new(tag: Tag, controller: u8, low: f32, mid: f32, high: f32) -> Self {
        Self {
            tag,
            controller,
            low,
            mid,
            high,
        }
    }

    pub fn controller(&self) -> u8 {
        self.controller
    }

    pub fn low(&self) -> f32 {
        self.low
    }

    pub fn set_low(&mut self, value: f32) {
        self.low = value;
    }

    pub fn mid(&self) -> f32 {
        self.mid
    }

    pub fn set_mid(&mut self, value: f32) {
        self.mid = value;
    }

    pub fn high(&self) -> f32 {
        self.high
    }

    pub fn set_high(&mut self, value: f32) {
        self.high = value;
    }

    pub fn value(&self, rack: &Rack) -> usize {
        match rack.controls[(self.tag, 0)] {
            Control::I(u) => u,
            c => panic!("Control must be I(usized) not {c:?}"),
        }
    }

    pub fn set_value(&self, rack: &mut Rack, value: usize) {
        rack.controls[(self.tag, 0)] = (value as f32).into();
    }

    pub fn map_range(&self, input: f32) -> f32 {
        let x = input / 127.0;
        interp(self.low, self.mid, self.high, x)
    }
}

impl Signal for MidiControl {
    tag!();

    fn signal(&self, rack: &mut Rack, _sample_rate: f32) {
        let value = self.value(rack);
        rack.outputs[(self.tag, 0)] = self.map_range(value as f32);
    }
}

#[derive(Debug, Copy, Clone)]
pub struct MidiControlBuilder {
    controller: u8,
    low: f32,
    mid: f32,
    high: f32,
    value: Control,
}

impl MidiControlBuilder {
    pub fn new(controller: u8) -> Self {
        Self {
            controller,
            low: 0.0,
            mid: 0.5,
            high: 1.0,
            value: 0.0.into(),
        }
    }

    build!(value);

    pub fn low(&mut self, value: f32) -> &mut Self {
        self.low = value;
        self
    }

    pub fn mid(&mut self, value: f32) -> &mut Self {
        self.mid = value;
        self
    }

    pub fn high(&mut self, value: f32) -> &mut Self {
        self.high = value;
        self
    }

    pub fn rack(&self, rack: &mut Rack) -> Arc<MidiControl> {
        let n = rack.num_modules();
        rack.controls[(n, 0)] = self.value;
        let mc = Arc::new(MidiControl::new(
            n.into(),
            self.controller,
            self.low,
            self.mid,
            self.high,
        ));
        rack.push(mc.clone());
        mc
    }
}

pub fn listen_midi(midi_sender: Sender<Vec<u8>>) -> Result<(), Box<dyn Error>> {
    let mut input = String::new();
    let mut midi_in = MidiInput::new("midir reading input")?;
    midi_in.ignore(Ignore::None);

    // Get an input port (read from console if multiple are available)
    let in_ports = midi_in.ports();
    let in_port = match in_ports.len() {
        0 => return Err("no input port found".into()),
        1 => {
            println!(
                "Choosing the only available input port: {}",
                midi_in.port_name(&in_ports[0]).unwrap()
            );
            &in_ports[0]
        }
        _ => {
            println!("\nAvailable input ports:");
            for (i, p) in in_ports.iter().enumerate() {
                println!("{}: {}", i, midi_in.port_name(p).unwrap());
            }
            print!("Please select input port: ");
            stdout().flush()?;
            let mut input = String::new();
            stdin().read_line(&mut input)?;
            in_ports
                .get(input.trim().parse::<usize>()?)
                .ok_or("invalid input port selected")?
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
