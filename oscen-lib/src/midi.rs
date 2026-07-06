use crate::graph::{EventInput, EventInstance, EventOutput, EventPayload, SignalProcessor};
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

impl NoteOnEvent {
    /// Extract a note-on from an event payload.
    ///
    /// Accepts both the allocation-free `EventPayload::Midi` representation
    /// (a note-on status byte with non-zero velocity) and a boxed
    /// `NoteOnEvent` object.
    pub fn from_payload(payload: &EventPayload) -> Option<Self> {
        match payload {
            EventPayload::Midi(bytes) => {
                if bytes[0] & 0xF0 == 0x90 && bytes[2] > 0 {
                    Some(Self {
                        note: bytes[1],
                        velocity: (bytes[2] as f32 / 127.0).clamp(0.0, 1.0),
                    })
                } else {
                    None
                }
            }
            EventPayload::Object(obj) => obj.as_any().downcast_ref::<Self>().copied(),
            EventPayload::Scalar(_) => None,
        }
    }
}

/// Note-off event with note number
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NoteOffEvent {
    pub note: u8,
}

impl NoteOffEvent {
    /// Extract a note-off from an event payload.
    ///
    /// Accepts both the allocation-free `EventPayload::Midi` representation
    /// (a note-off status byte, or note-on with velocity 0) and a boxed
    /// `NoteOffEvent` object.
    pub fn from_payload(payload: &EventPayload) -> Option<Self> {
        match payload {
            EventPayload::Midi(bytes) => match bytes[0] & 0xF0 {
                0x80 => Some(Self { note: bytes[1] }),
                0x90 if bytes[2] == 0 => Some(Self { note: bytes[1] }),
                _ => None,
            },
            EventPayload::Object(obj) => obj.as_any().downcast_ref::<Self>().copied(),
            EventPayload::Scalar(_) => None,
        }
    }
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
    // Event handlers called automatically by derive macro via process_event_inputs()
    fn on_note_on(&mut self, event: &EventInstance) {
        if let Some(note_on) = NoteOnEvent::from_payload(&event.payload) {
            self.current_note = Some(note_on.note);
            self.current_frequency = Self::midi_note_to_freq(note_on.note);

            // Emit gate-on event with velocity - push directly to EventOutput field
            let _ = self.gate.try_push(EventInstance {
                frame_offset: event.frame_offset,
                payload: EventPayload::Scalar(note_on.velocity),
            });
        }
    }

    fn on_note_off(&mut self, event: &EventInstance) {
        if let Some(note_off) = NoteOffEvent::from_payload(&event.payload) {
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
                    Some(ParsedMidi::NoteOn { note, velocity })
                }
            }
            _ => None,
        }
    }
}

/// Internal enum for parsed MIDI messages
enum ParsedMidi {
    NoteOn { note: u8, velocity: u8 },
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
    // Event handler called automatically by derive macro via process_event_inputs()
    fn on_midi_in(&mut self, event: &EventInstance) {
        // Accept both the allocation-free Midi payload and the legacy
        // Object(RawMidiMessage) representation.
        let parsed = match &event.payload {
            EventPayload::Midi(bytes) => Self::parse_bytes(bytes),
            EventPayload::Object(obj) => obj
                .as_any()
                .downcast_ref::<RawMidiMessage>()
                .and_then(|raw| Self::parse_bytes(&raw.bytes[..raw.len])),
            EventPayload::Scalar(_) => None,
        };

        // Emit plain-data Midi payloads: no heap allocation on the audio thread.
        match parsed {
            Some(ParsedMidi::NoteOn { note, velocity }) => {
                let _ = self.note_on.try_push(EventInstance {
                    frame_offset: event.frame_offset,
                    payload: EventPayload::Midi([0x90, note, velocity]),
                });
            }
            Some(ParsedMidi::NoteOff { note }) => {
                let _ = self.note_off.try_push(EventInstance {
                    frame_offset: event.frame_offset,
                    payload: EventPayload::Midi([0x80, note, 0]),
                });
            }
            None => {}
        }
    }
}

/// Helper function to create a raw MIDI message event payload.
/// Complete 3-byte messages use the allocation-free `EventPayload::Midi`
/// representation; shorter messages fall back to a boxed `RawMidiMessage`.
pub fn raw_midi_event(bytes: &[u8]) -> EventPayload {
    let msg = RawMidiMessage::new(bytes);
    if msg.len == 3 {
        EventPayload::Midi(msg.bytes)
    } else {
        EventPayload::Object(Arc::new(msg))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_note_to_frequency_conversion() {
        assert_eq!(MidiVoiceHandler::midi_note_to_freq(69), 440.0); // A4
        assert!((MidiVoiceHandler::midi_note_to_freq(60) - 261.626).abs() < 0.01); // C4
        assert!((MidiVoiceHandler::midi_note_to_freq(81) - 880.0).abs() < 0.01);
        // A5
    }

    #[test]
    fn test_midi_parser_parse_note_on() {
        let parsed = MidiParser::parse_bytes(&[0x90, 60, 100]);
        assert!(matches!(
            parsed,
            Some(ParsedMidi::NoteOn {
                note: 60,
                velocity: 100
            })
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

    #[test]
    fn test_raw_midi_event_is_pod_for_full_messages() {
        assert_eq!(
            raw_midi_event(&[0x90, 60, 100]).as_midi(),
            Some([0x90, 60, 100])
        );
        // Short messages fall back to the boxed representation
        assert!(raw_midi_event(&[0xC0, 5]).as_object().is_some());
    }

    fn midi_event(bytes: [u8; 3]) -> EventInstance {
        EventInstance {
            frame_offset: 0,
            payload: EventPayload::Midi(bytes),
        }
    }

    #[test]
    fn test_note_on_off_round_trip_with_pod_payload() {
        use float_cmp::approx_eq;

        let mut parser = MidiParser::new();
        let mut handler = MidiVoiceHandler::new();

        // Note-on A4 (69), velocity 100, through the parser
        parser.on_midi_in(&midi_event([0x90, 69, 100]));
        assert_eq!(parser.note_on.len(), 1);
        let note_on = parser.note_on.iter().next().unwrap().clone();
        assert_eq!(note_on.payload.as_midi(), Some([0x90, 69, 100]));

        handler.on_note_on(&note_on);
        handler.process();
        assert!(approx_eq!(f32, handler.frequency, 440.0, ulps = 2));
        let gate_on = handler.gate.iter().next().unwrap();
        assert!(approx_eq!(
            f32,
            gate_on.payload.as_scalar().unwrap(),
            100.0 / 127.0,
            ulps = 2
        ));
        handler.gate.clear();

        // Note-off for the same note
        parser.on_midi_in(&midi_event([0x80, 69, 0]));
        assert_eq!(parser.note_off.len(), 1);
        let note_off = parser.note_off.iter().next().unwrap().clone();
        assert_eq!(note_off.payload.as_midi(), Some([0x80, 69, 0]));

        handler.on_note_off(&note_off);
        let gate_off = handler.gate.iter().next().unwrap();
        assert!(approx_eq!(
            f32,
            gate_off.payload.as_scalar().unwrap(),
            0.0,
            ulps = 2
        ));
    }

    #[test]
    fn test_parser_accepts_boxed_raw_midi_message() {
        let mut parser = MidiParser::new();
        parser.on_midi_in(&EventInstance {
            frame_offset: 0,
            payload: EventPayload::Object(Arc::new(RawMidiMessage::new(&[0x90, 60, 64]))),
        });
        let note_on = parser.note_on.iter().next().unwrap();
        assert_eq!(note_on.payload.as_midi(), Some([0x90, 60, 64]));
    }

    #[test]
    fn test_voice_handler_accepts_boxed_note_events() {
        use float_cmp::approx_eq;

        let mut handler = MidiVoiceHandler::new();
        handler.on_note_on(&EventInstance {
            frame_offset: 0,
            payload: EventPayload::Object(Arc::new(NoteOnEvent {
                note: 69,
                velocity: 0.5,
            })),
        });
        handler.process();
        assert!(approx_eq!(f32, handler.frequency, 440.0, ulps = 2));

        handler.on_note_off(&EventInstance {
            frame_offset: 0,
            payload: EventPayload::Object(Arc::new(NoteOffEvent { note: 69 })),
        });
        let gate_off = handler.gate.iter().last().unwrap();
        assert!(approx_eq!(
            f32,
            gate_off.payload.as_scalar().unwrap(),
            0.0,
            ulps = 2
        ));
    }

    #[test]
    fn test_note_events_from_payload() {
        use float_cmp::approx_eq;

        let note_on = NoteOnEvent::from_payload(&EventPayload::Midi([0x90, 60, 127])).unwrap();
        assert_eq!(note_on.note, 60);
        assert!(approx_eq!(f32, note_on.velocity, 1.0, ulps = 2));

        // Note-on with velocity 0 is a note-off, not a note-on
        assert!(NoteOnEvent::from_payload(&EventPayload::Midi([0x90, 60, 0])).is_none());
        assert_eq!(
            NoteOffEvent::from_payload(&EventPayload::Midi([0x90, 60, 0])),
            Some(NoteOffEvent { note: 60 })
        );
        assert_eq!(
            NoteOffEvent::from_payload(&EventPayload::Midi([0x80, 60, 64])),
            Some(NoteOffEvent { note: 60 })
        );

        // Scalars and unrelated statuses are ignored
        assert!(NoteOnEvent::from_payload(&EventPayload::Scalar(1.0)).is_none());
        assert!(NoteOffEvent::from_payload(&EventPayload::Midi([0xB0, 1, 64])).is_none());
    }
}
