use crate::graph::{
    EventInstance, EventPayload, InputEndpoint, NodeKey, ProcessingContext, ProcessingNode,
    SignalProcessor, ValueKey,
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
    note_on: (),

    #[input(event)]
    note_off: (),

    #[output(value)]
    frequency: f32,

    #[output(event)]
    gate: (),

    current_note: Option<u8>,
    current_frequency: f32,
}

impl MidiVoiceHandler {
    pub fn new() -> Self {
        Self {
            note_on: (),
            note_off: (),
            frequency: 440.0,
            gate: (),
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
    fn process(&mut self, _sample_rate: f32) {
        // Update frequency output
        // Event handling is done via on_note_on/on_note_off handlers
        self.frequency = self.current_frequency;
    }
}

impl MidiVoiceHandler {
    // Event handlers called automatically by macro-generated NodeIO
    fn on_note_on(&mut self, event: &EventInstance, context: &mut ProcessingContext) {
        if let EventPayload::Object(obj) = &event.payload {
            if let Some(note_on) = obj.as_any().downcast_ref::<NoteOnEvent>() {
                self.current_note = Some(note_on.note);
                self.current_frequency = Self::midi_note_to_freq(note_on.note);

                // Emit gate-on event with velocity (gate is output index 1)
                context.emit_scalar_event(1, event.frame_offset, note_on.velocity);
            }
        }
    }

    fn on_note_off(&mut self, event: &EventInstance, context: &mut ProcessingContext) {
        if let EventPayload::Object(obj) = &event.payload {
            if let Some(note_off) = obj.as_any().downcast_ref::<NoteOffEvent>() {
                // Only turn off gate if this is the current note
                if self.current_note == Some(note_off.note) {
                    // Emit gate-off event (gate is output index 1)
                    context.emit_scalar_event(1, event.frame_offset, 0.0);
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
    midi_in: (),

    #[output(event)]
    note_on: (),

    #[output(event)]
    note_off: (),
}

impl MidiParser {
    pub fn new() -> Self {
        Self {
            midi_in: (),
            note_on: (),
            note_off: (),
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
    fn process(&mut self, _sample_rate: f32) {
        // All event processing is done via on_midi_in handler
        // This node has no stream outputs to update
    }
}

impl MidiParser {
    // Event handler called automatically by macro-generated NodeIO
    fn on_midi_in(&mut self, event: &EventInstance, context: &mut ProcessingContext) {
        if let EventPayload::Object(obj) = &event.payload {
            // Try to downcast to RawMidiMessage
            if let Some(raw_midi) = obj.as_any().downcast_ref::<RawMidiMessage>() {
                // Parse the raw bytes
                if let Some(parsed) = Self::parse_bytes(&raw_midi.bytes[..raw_midi.len]) {
                    match parsed {
                        ParsedMidi::NoteOn { note, velocity } => {
                            // Emit note-on event (output index 0)
                            let note_on_payload =
                                EventPayload::Object(Arc::new(NoteOnEvent { note, velocity }));
                            context.emit_timed_event(0, event.frame_offset, note_on_payload);
                        }
                        ParsedMidi::NoteOff { note } => {
                            // Emit note-off event (output index 1)
                            let note_off_payload =
                                EventPayload::Object(Arc::new(NoteOffEvent { note }));
                            context.emit_timed_event(1, event.frame_offset, note_off_payload);
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

/// Queue raw MIDI bytes to a MidiParser input
/// Returns true if the event was successfully queued
pub fn queue_raw_midi<I>(
    graph: &mut crate::Graph,
    midi_input: I,
    frame_offset: u32,
    bytes: &[u8],
) -> bool
where
    I: Into<InputEndpoint>,
{
    graph.queue_event(midi_input, frame_offset, raw_midi_event(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Graph;

    #[test]
    fn test_midi_voice_handler_with_typed_events() {
        let mut graph = Graph::new(44100.0);
        let voice = graph.add_node(MidiVoiceHandler::new());

        // Send note-on event for middle C (note 60)
        let note_on_payload = EventPayload::Object(Arc::new(NoteOnEvent {
            note: 60,
            velocity: 0.8,
        }));
        assert!(graph.queue_event(voice.note_on, 0, note_on_payload));

        // Process
        graph.process().expect("graph processes");

        // Check frequency output (middle C should be ~261.63 Hz)
        let freq = graph.get_value(&voice.frequency).unwrap();
        assert!((freq - 261.626).abs() < 0.01);

        // Check gate event was emitted
        let mut gate_events = Vec::new();
        graph.drain_events(voice.gate, |event| {
            gate_events.push(event.clone());
        });
        assert_eq!(gate_events.len(), 1);
        match gate_events[0].payload {
            EventPayload::Scalar(v) => assert_eq!(v, 0.8),
            _ => panic!("expected scalar gate event"),
        }

        // Send note-off event
        let note_off_payload = EventPayload::Object(Arc::new(NoteOffEvent { note: 60 }));
        assert!(graph.queue_event(voice.note_off, 0, note_off_payload));

        // Process
        graph.process().expect("graph processes");

        // Check gate-off event was emitted
        let mut gate_events = Vec::new();
        graph.drain_events(voice.gate, |event| {
            gate_events.push(event.clone());
        });
        assert_eq!(gate_events.len(), 1);
        match gate_events[0].payload {
            EventPayload::Scalar(v) => assert_eq!(v, 0.0),
            _ => panic!("expected scalar gate event"),
        }
    }

    #[test]
    fn test_note_to_frequency_conversion() {
        assert_eq!(MidiVoiceHandler::midi_note_to_freq(69), 440.0); // A4
        assert!((MidiVoiceHandler::midi_note_to_freq(60) - 261.626).abs() < 0.01); // C4
        assert!((MidiVoiceHandler::midi_note_to_freq(81) - 880.0).abs() < 0.01);
        // A5
    }

    #[test]
    fn test_midi_parser() {
        let mut graph = Graph::new(44100.0);
        let parser = graph.add_node(MidiParser::new());

        // Send raw MIDI note-on message (0x90 = note-on, 60 = middle C, 100 = velocity)
        assert!(queue_raw_midi(
            &mut graph,
            parser.midi_in,
            0,
            &[0x90, 60, 100]
        ));

        // Process
        graph.process().expect("graph processes");

        // Check that note-on event was emitted
        let mut note_on_events = Vec::new();
        graph.drain_events(parser.note_on, |event| {
            note_on_events.push(event.clone());
        });
        assert_eq!(note_on_events.len(), 1);
        match &note_on_events[0].payload {
            EventPayload::Object(obj) => {
                let note_on = obj.as_any().downcast_ref::<NoteOnEvent>().unwrap();
                assert_eq!(note_on.note, 60);
                assert!((note_on.velocity - 100.0 / 127.0).abs() < 0.01);
            }
            _ => panic!("expected object payload"),
        }

        // Send raw MIDI note-off message (0x80 = note-off, 60 = middle C)
        assert!(queue_raw_midi(
            &mut graph,
            parser.midi_in,
            0,
            &[0x80, 60, 0]
        ));

        // Process
        graph.process().expect("graph processes");

        // Check that note-off event was emitted
        let mut note_off_events = Vec::new();
        graph.drain_events(parser.note_off, |event| {
            note_off_events.push(event.clone());
        });
        assert_eq!(note_off_events.len(), 1);
        match &note_off_events[0].payload {
            EventPayload::Object(obj) => {
                let note_off = obj.as_any().downcast_ref::<NoteOffEvent>().unwrap();
                assert_eq!(note_off.note, 60);
            }
            _ => panic!("expected object payload"),
        }
    }

    #[test]
    fn test_midi_parser_to_voice_handler() {
        let mut graph = Graph::new(44100.0);
        let parser = graph.add_node(MidiParser::new());
        let voice = graph.add_node(MidiVoiceHandler::new());

        // Connect parser outputs to voice handler inputs
        graph.connect(parser.note_on, voice.note_on);
        graph.connect(parser.note_off, voice.note_off);

        // Send raw MIDI note-on
        assert!(queue_raw_midi(
            &mut graph,
            parser.midi_in,
            0,
            &[0x90, 60, 100]
        ));

        // Process
        graph.process().expect("graph processes");

        // Check frequency output
        let freq = graph.get_value(&voice.frequency).unwrap();
        assert!((freq - 261.626).abs() < 0.01);

        // Check gate event was emitted
        let mut gate_events = Vec::new();
        graph.drain_events(voice.gate, |event| {
            gate_events.push(event.clone());
        });
        assert_eq!(gate_events.len(), 1);

        // Send raw MIDI note-off
        assert!(queue_raw_midi(
            &mut graph,
            parser.midi_in,
            0,
            &[0x80, 60, 0]
        ));

        // Process
        graph.process().expect("graph processes");

        // Check gate-off event was emitted
        let mut gate_events = Vec::new();
        graph.drain_events(voice.gate, |event| {
            gate_events.push(event.clone());
        });
        assert_eq!(gate_events.len(), 1);
        match gate_events[0].payload {
            EventPayload::Scalar(v) => assert_eq!(v, 0.0),
            _ => panic!("expected scalar gate event"),
        }
    }
}
