use crate::graph::{
    EventInput, EventInstance, EventOutput, EventPayload, NodeKey, ProcessingNode, SignalProcessor,
    ValueKey,
};
use crate::Node;
use std::sync::Arc;

/// Raw MIDI message containing up to 3 bytes
/// Used to pass unparsed MIDI data into the graph for processing by MidiParser nodes
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RawMidiMessage {
    pub bytes: [u8; 3],
    pub len: usize,
}

impl RawMidiMessage {
    pub fn new(bytes: &[u8]) -> Self {
        let mut msg = Self {
            bytes: [0, 0, 0],
            len: bytes.len().min(3),
        };
        msg.bytes[..msg.len].copy_from_slice(&bytes[..msg.len]);
        msg
    }
}

/// Note-on event with note number and velocity
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NoteOnEvent {
    pub note: u8,
    pub velocity: f32, // 0.0 - 1.0
}

/// Note-off event with note number
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NoteOffEvent {
    pub note: u8,
}

/// A node that manages MIDI note state and converts to frequency/gate outputs.
/// Handles note-on/note-off events and outputs the current frequency and gate events.
#[derive(Debug, Node)]
pub struct MidiVoiceHandler {
    #[input(event)]
    pub note_on: EventInput,

    #[input(event)]
    pub note_off: EventInput,

    #[output(value)]
    pub frequency: f32,

    #[output(event)]
    pub gate: EventOutput,

    current_note: Option<u8>,
    current_frequency: f32,
}

impl MidiVoiceHandler {
    pub fn new() -> Self {
        Self {
            note_on: EventInput::default(),
            note_off: EventInput::default(),
            frequency: 440.0,
            gate: EventOutput::default(),
            current_note: None,
            current_frequency: 440.0,
        }
    }

    fn midi_note_to_freq(note: u8) -> f32 {
        let semitone_offset = note as f32 - 69.0;
        440.0 * 2f32.powf(semitone_offset / 12.0)
    }
}

impl Default for MidiVoiceHandler {
    fn default() -> Self {
        Self::new()
    }
}

impl SignalProcessor for MidiVoiceHandler {
    #[inline(always)]
    fn process(&mut self) {
        // Update frequency output
        // Event handling is done via on_note_on/on_note_off handlers
        self.frequency = self.current_frequency;
    }
}

impl MidiVoiceHandler {
    // Event handlers called automatically by macro-generated NodeIO
    fn on_note_on(&mut self, event: &EventInstance) {
        if let EventPayload::Object(obj) = &event.payload {
            if let Some(note_on) = obj.as_any().downcast_ref::<NoteOnEvent>() {
                self.current_note = Some(note_on.note);
                self.current_frequency = Self::midi_note_to_freq(note_on.note);

                // Emit gate-on event with velocity - push directly to EventOutput field
                let _ = self.gate.try_push(EventInstance {
                    frame_offset: event.frame_offset,
                    payload: EventPayload::Scalar(note_on.velocity),
                });
            }
        }
    }

    fn on_note_off(&mut self, event: &EventInstance) {
        if let EventPayload::Object(obj) = &event.payload {
            if let Some(note_off) = obj.as_any().downcast_ref::<NoteOffEvent>() {
                // Only turn off gate if this is the current note
                if self.current_note == Some(note_off.note) {
                    // Emit gate-off event - push directly to EventOutput field
                    let _ = self.gate.try_push(EventInstance {
                        frame_offset: event.frame_offset,
                        payload: EventPayload::Scalar(0.0),
                    });
                    self.current_note = None;
                }
            }
        }
    }
}

/// A node that parses raw MIDI messages and emits typed note events.
#[derive(Debug, Node)]
pub struct MidiParser {
    #[input(event)]
    pub midi_in: EventInput,

    #[output(event)]
    pub note_on: EventOutput,

    #[output(event)]
    pub note_off: EventOutput,
}

impl MidiParser {
    pub fn new() -> Self {
        Self {
            midi_in: EventInput::default(),
            note_on: EventOutput::default(),
            note_off: EventOutput::default(),
        }
    }

    /// Parse raw MIDI bytes and return parsed event type
    fn parse_bytes(data: &[u8]) -> Option<ParsedMidi> {
        if data.len() < 3 {
            return None;
        }

        let status = data[0] & 0xF0;
        let note = data[1];
        let velocity = data[2];

        match status {
            0x80 => Some(ParsedMidi::NoteOff { note }),
            0x90 => {
                if velocity == 0 {
                    // Note-on with velocity 0 is treated as note-off
                    Some(ParsedMidi::NoteOff { note })
                } else {
                    Some(ParsedMidi::NoteOn {
                        note,
                        velocity: (velocity as f32 / 127.0).clamp(0.0, 1.0),
                    })
                }
            }
            _ => None,
        }
    }
}

/// Internal enum for parsed MIDI messages
enum ParsedMidi {
    NoteOn { note: u8, velocity: f32 },
    NoteOff { note: u8 },
}

impl Default for MidiParser {
    fn default() -> Self {
        Self::new()
    }
}

impl SignalProcessor for MidiParser {
    #[inline(always)]
    fn process(&mut self) {
        // All event processing is done via on_midi_in handler
        // This node has no stream outputs to update
    }
}

impl MidiParser {
    // Event handler called automatically by macro-generated NodeIO
    fn on_midi_in(&mut self, event: &EventInstance) {
        if let EventPayload::Object(obj) = &event.payload {
            // Try to downcast to RawMidiMessage
            if let Some(raw_midi) = obj.as_any().downcast_ref::<RawMidiMessage>() {
                // Parse the raw bytes
                if let Some(parsed) = Self::parse_bytes(&raw_midi.bytes[..raw_midi.len]) {
                    match parsed {
                        ParsedMidi::NoteOn { note, velocity } => {
                            // Push note-on event directly to EventOutput field
                            let _ = self.note_on.try_push(EventInstance {
                                frame_offset: event.frame_offset,
                                payload: EventPayload::Object(Arc::new(NoteOnEvent { note, velocity })),
                            });
                        }
                        ParsedMidi::NoteOff { note } => {
                            // Push note-off event directly to EventOutput field
                            let _ = self.note_off.try_push(EventInstance {
                                frame_offset: event.frame_offset,
                                payload: EventPayload::Object(Arc::new(NoteOffEvent { note })),
                            });
                        }
                    }
                }
            }
        }
    }
}

/// Helper function to create a raw MIDI message event payload
pub fn raw_midi_event(bytes: &[u8]) -> EventPayload {
    EventPayload::Object(Arc::new(RawMidiMessage::new(bytes)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_note_to_frequency_conversion() {
        assert_eq!(MidiVoiceHandler::midi_note_to_freq(69), 440.0); // A4
        assert!((MidiVoiceHandler::midi_note_to_freq(60) - 261.626).abs() < 0.01); // C4
        assert!((MidiVoiceHandler::midi_note_to_freq(81) - 880.0).abs() < 0.01); // A5
    }

    #[test]
    fn test_midi_parser_parse_note_on() {
        let parsed = MidiParser::parse_bytes(&[0x90, 60, 100]);
        assert!(matches!(
            parsed,
            Some(ParsedMidi::NoteOn { note: 60, velocity }) if (velocity - 100.0/127.0).abs() < 0.01
        ));
    }

    #[test]
    fn test_midi_parser_parse_note_off() {
        let parsed = MidiParser::parse_bytes(&[0x80, 60, 0]);
        assert!(matches!(parsed, Some(ParsedMidi::NoteOff { note: 60 })));
    }

    #[test]
    fn test_midi_parser_note_on_velocity_zero_is_note_off() {
        // Note-on with velocity 0 should be treated as note-off
        let parsed = MidiParser::parse_bytes(&[0x90, 60, 0]);
        assert!(matches!(parsed, Some(ParsedMidi::NoteOff { note: 60 })));
    }

    #[test]
    fn test_raw_midi_message() {
        let msg = RawMidiMessage::new(&[0x90, 60, 100]);
        assert_eq!(msg.bytes[0], 0x90);
        assert_eq!(msg.bytes[1], 60);
        assert_eq!(msg.bytes[2], 100);
        assert_eq!(msg.len, 3);
    }
}
