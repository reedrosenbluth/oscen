use anyhow::{Context, Result};
use midir::{MidiInput, MidiInputConnection};
use std::sync::mpsc::Sender;

/// A raw MIDI message with its bytes
#[derive(Debug, Clone)]
pub struct RawMidiBytes {
    pub bytes: Vec<u8>,
}

pub struct MidiConnection {
    _connections: Vec<MidiInputConnection<()>>,
}

impl MidiConnection {
    pub fn new(tx: Sender<RawMidiBytes>) -> Result<Self> {
        let midi_in = MidiInput::new("pivot-midi-input").context("failed to create MIDI input")?;

        let ports = midi_in.ports();
        if ports.is_empty() {
            println!("No MIDI sources detected. Connect a device to trigger notes.");
            return Ok(Self {
                _connections: Vec::new(),
            });
        }

        let mut connections = Vec::new();
        for (i, port) in ports.iter().enumerate() {
            let midi_in =
                MidiInput::new("pivot-midi-input").context("failed to create MIDI input")?;

            let port_name = midi_in
                .port_name(port)
                .unwrap_or_else(|_| format!("Port {}", i));
            println!("Connecting to MIDI source: {}", port_name);

            let tx_clone = tx.clone();
            let connection = midi_in
                .connect(
                    port,
                    &format!("pivot-{}", port_name),
                    move |_timestamp, message, _| {
                        let _ = tx_clone.send(RawMidiBytes {
                            bytes: message.to_vec(),
                        });
                    },
                    (),
                )
                .context("failed to connect to MIDI port")?;

            connections.push(connection);
        }

        Ok(Self {
            _connections: connections,
        })
    }
}
