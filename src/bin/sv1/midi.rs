use crossbeam::crossbeam_channel::Sender;
use midir::{Ignore, MidiInput};
use std::error::Error;
use std::io::{stdin, stdout, Write};

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
            println!("{:?} (len = {})", message, message.len());
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
