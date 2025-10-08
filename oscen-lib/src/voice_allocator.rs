use crate::graph::{EventPayload, ProcessingContext, ProcessingNode, SignalProcessor};
use crate::midi::{NoteOffEvent, NoteOnEvent};

const MAX_VOICES: usize = 8;

#[derive(Debug, Clone, Copy)]
struct VoiceState {
    active: bool,
    note: Option<u8>,
    age: u32, // For voice stealing - higher age = older
}

impl VoiceState {
    fn new() -> Self {
        Self {
            active: false,
            note: None,
            age: 0,
        }
    }
}

/// Voice allocator that distributes note events across multiple voices
/// Implements round-robin allocation with voice stealing when all voices are busy
#[derive(Debug)]
pub struct VoiceAllocator<const NUM_VOICES: usize> {
    voices: [VoiceState; MAX_VOICES],
    next_voice: usize,
    current_age: u32,
}

impl<const NUM_VOICES: usize> VoiceAllocator<NUM_VOICES> {
    pub fn new() -> Self {
        assert!(NUM_VOICES <= MAX_VOICES, "NUM_VOICES must be <= MAX_VOICES");
        Self {
            voices: [VoiceState::new(); MAX_VOICES],
            next_voice: 0,
            current_age: 0,
        }
    }

    /// Find a voice to allocate for a new note
    fn allocate_voice(&mut self, note: u8) -> usize {
        // First, try to find an inactive voice
        for i in 0..NUM_VOICES {
            if !self.voices[i].active {
                self.voices[i].active = true;
                self.voices[i].note = Some(note);
                self.voices[i].age = self.current_age;
                self.current_age += 1;
                return i;
            }
        }

        // All voices active - steal the oldest voice (voice stealing)
        let oldest_voice = (0..NUM_VOICES)
            .min_by_key(|&i| self.voices[i].age)
            .unwrap_or(0);

        self.voices[oldest_voice].note = Some(note);
        self.voices[oldest_voice].age = self.current_age;
        self.current_age += 1;

        oldest_voice
    }

    /// Find which voice is playing a specific note
    fn find_voice_for_note(&self, note: u8) -> Option<usize> {
        (0..NUM_VOICES).find(|&i| self.voices[i].active && self.voices[i].note == Some(note))
    }

    /// Release a voice
    fn release_voice(&mut self, voice_idx: usize) {
        if voice_idx < NUM_VOICES {
            self.voices[voice_idx].active = false;
            self.voices[voice_idx].note = None;
        }
    }
}

impl<const NUM_VOICES: usize> Default for VoiceAllocator<NUM_VOICES> {
    fn default() -> Self {
        Self::new()
    }
}

// Type alias for 4-voice allocator (makes it easier to use with the graph macro)
pub type VoiceAllocator4 = VoiceAllocator<4>;

impl SignalProcessor for VoiceAllocator<4> {
    fn process(&mut self, _sample_rate: f32, context: &mut ProcessingContext) -> f32 {
        // Process note-on events
        let note_on_events: Vec<_> = context.events(0).iter().cloned().collect();
        for event in note_on_events {
            if let EventPayload::Object(obj) = &event.payload {
                if let Some(note_on) = obj.as_any().downcast_ref::<NoteOnEvent>() {
                    let voice_idx = self.allocate_voice(note_on.note);

                    // Emit note-on to the allocated voice (output index = voice_idx)
                    context.emit_event(voice_idx, event);
                }
            }
        }

        // Process note-off events
        let note_off_events: Vec<_> = context.events(1).iter().cloned().collect();
        for event in note_off_events {
            if let EventPayload::Object(obj) = &event.payload {
                if let Some(note_off) = obj.as_any().downcast_ref::<NoteOffEvent>() {
                    if let Some(voice_idx) = self.find_voice_for_note(note_off.note) {
                        // Emit note-off to the voice playing this note
                        context.emit_event(voice_idx, event);
                        self.release_voice(voice_idx);
                    }
                }
            }
        }

        0.0 // VoiceAllocator doesn't output audio
    }
}

// Manually implement ProcessingNode for VoiceAllocator<4>
impl ProcessingNode for VoiceAllocator<4> {
    type Endpoints = VoiceAllocator4Endpoints;

    const ENDPOINT_DESCRIPTORS: &'static [crate::graph::EndpointDescriptor] = &[
        crate::graph::EndpointDescriptor::new(
            "note_on",
            crate::graph::EndpointType::Event,
            crate::graph::EndpointDirection::Input,
        ),
        crate::graph::EndpointDescriptor::new(
            "note_off",
            crate::graph::EndpointType::Event,
            crate::graph::EndpointDirection::Input,
        ),
        crate::graph::EndpointDescriptor::new(
            "voice_0",
            crate::graph::EndpointType::Event,
            crate::graph::EndpointDirection::Output,
        ),
        crate::graph::EndpointDescriptor::new(
            "voice_1",
            crate::graph::EndpointType::Event,
            crate::graph::EndpointDirection::Output,
        ),
        crate::graph::EndpointDescriptor::new(
            "voice_2",
            crate::graph::EndpointType::Event,
            crate::graph::EndpointDirection::Output,
        ),
        crate::graph::EndpointDescriptor::new(
            "voice_3",
            crate::graph::EndpointType::Event,
            crate::graph::EndpointDirection::Output,
        ),
    ];

    fn create_endpoints(
        node_key: crate::NodeKey,
        inputs: arrayvec::ArrayVec<crate::ValueKey, { crate::graph::MAX_NODE_ENDPOINTS }>,
        outputs: arrayvec::ArrayVec<crate::ValueKey, { crate::graph::MAX_NODE_ENDPOINTS }>,
    ) -> Self::Endpoints {
        VoiceAllocator4Endpoints {
            node_key,
            note_on: crate::EventInput::new(crate::graph::InputEndpoint::new(inputs[0])),
            note_off: crate::EventInput::new(crate::graph::InputEndpoint::new(inputs[1])),
            voice_0: crate::EventOutput::new(outputs[0]),
            voice_1: crate::EventOutput::new(outputs[1]),
            voice_2: crate::EventOutput::new(outputs[2]),
            voice_3: crate::EventOutput::new(outputs[3]),
        }
    }
}

#[derive(Debug)]
pub struct VoiceAllocator4Endpoints {
    node_key: crate::NodeKey,
    note_on: crate::EventInput,
    note_off: crate::EventInput,
    voice_0: crate::EventOutput,
    voice_1: crate::EventOutput,
    voice_2: crate::EventOutput,
    voice_3: crate::EventOutput,
}

impl VoiceAllocator4Endpoints {
    pub fn note_on(&self) -> crate::EventInput {
        self.note_on
    }

    pub fn note_off(&self) -> crate::EventInput {
        self.note_off
    }

    pub fn voice_0(&self) -> crate::EventOutput {
        self.voice_0
    }

    pub fn voice_1(&self) -> crate::EventOutput {
        self.voice_1
    }

    pub fn voice_2(&self) -> crate::EventOutput {
        self.voice_2
    }

    pub fn voice_3(&self) -> crate::EventOutput {
        self.voice_3
    }

    pub fn node_key(&self) -> crate::NodeKey {
        self.node_key
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_voice_allocation() {
        let mut allocator = VoiceAllocator::<4>::new();

        // Allocate 4 notes
        let voice0 = allocator.allocate_voice(60);
        let voice1 = allocator.allocate_voice(64);
        let voice2 = allocator.allocate_voice(67);
        let voice3 = allocator.allocate_voice(72);

        assert_eq!(voice0, 0);
        assert_eq!(voice1, 1);
        assert_eq!(voice2, 2);
        assert_eq!(voice3, 3);

        // All voices should be active
        assert!(allocator.voices[0].active);
        assert!(allocator.voices[1].active);
        assert!(allocator.voices[2].active);
        assert!(allocator.voices[3].active);
    }

    #[test]
    fn test_voice_stealing() {
        let mut allocator = VoiceAllocator::<4>::new();

        // Allocate 4 voices
        allocator.allocate_voice(60);
        allocator.allocate_voice(64);
        allocator.allocate_voice(67);
        allocator.allocate_voice(72);

        // Allocate a 5th note - should steal the oldest voice (voice 0)
        let stolen_voice = allocator.allocate_voice(76);
        assert_eq!(stolen_voice, 0);
        assert_eq!(allocator.voices[0].note, Some(76));
    }

    #[test]
    fn test_find_and_release_voice() {
        let mut allocator = VoiceAllocator::<4>::new();

        allocator.allocate_voice(60);
        allocator.allocate_voice(64);

        // Find voice playing note 64
        let voice_idx = allocator.find_voice_for_note(64);
        assert_eq!(voice_idx, Some(1));

        // Release it
        allocator.release_voice(1);
        assert!(!allocator.voices[1].active);

        // Should not be found anymore
        assert_eq!(allocator.find_voice_for_note(64), None);
    }
}
