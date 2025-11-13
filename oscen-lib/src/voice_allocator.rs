use crate::graph::{EventPayload, ProcessingContext, ProcessingNode, SignalProcessor};
use crate::midi::{NoteOffEvent, NoteOnEvent};

const MAX_VOICES: usize = 24;

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
    current_age: u32,
}

impl<const NUM_VOICES: usize> VoiceAllocator<NUM_VOICES> {
    pub fn new() -> Self {
        assert!(NUM_VOICES <= MAX_VOICES, "NUM_VOICES must be <= MAX_VOICES");
        Self {
            voices: [VoiceState::new(); MAX_VOICES],
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

// Type aliases for common voice counts
pub type VoiceAllocator2 = VoiceAllocator<2>;
pub type VoiceAllocator4 = VoiceAllocator<4>;

impl<const NUM_VOICES: usize> SignalProcessor for VoiceAllocator<NUM_VOICES> {
    fn process(&mut self, _sample_rate: f32) {
        // All event processing is done in NodeIO::read_inputs
        // This node has no stream outputs to update
    }
}

// Manual NodeIO implementation for VoiceAllocator
impl<const NUM_VOICES: usize> crate::graph::NodeIO for VoiceAllocator<NUM_VOICES> {
    fn read_inputs<'a>(&mut self, context: &mut ProcessingContext<'a>) {
        // Handle note_on events (input index 0)
        let note_on_slice = context.events(0);
        if !note_on_slice.is_empty() {
            use arrayvec::ArrayVec;
            // Collect events into stack-allocated buffer to avoid borrow checker issues
            let note_on_events: ArrayVec<_, 64> = note_on_slice.iter().cloned().collect();
            for event in note_on_events {
                if let EventPayload::Object(obj) = &event.payload {
                    if let Some(note_on) = obj.as_any().downcast_ref::<NoteOnEvent>() {
                        let voice_idx = self.allocate_voice(note_on.note);
                        context.emit_event(voice_idx, event);
                    }
                }
            }
        }

        // Handle note_off events (input index 1)
        let note_off_slice = context.events(1);
        if !note_off_slice.is_empty() {
            use arrayvec::ArrayVec;
            let note_off_events: ArrayVec<_, 64> = note_off_slice.iter().cloned().collect();
            for event in note_off_events {
                if let EventPayload::Object(obj) = &event.payload {
                    if let Some(note_off) = obj.as_any().downcast_ref::<NoteOffEvent>() {
                        if let Some(voice_idx) = self.find_voice_for_note(note_off.note) {
                            context.emit_event(voice_idx, event);
                            self.release_voice(voice_idx);
                        }
                    }
                }
            }
        }
    }
}

// Generic endpoints struct using arrays instead of separate fields
#[derive(Debug, Copy, Clone)]
pub struct VoiceAllocatorEndpoints<const NUM_VOICES: usize> {
    pub node_key: crate::NodeKey,
    pub note_on: crate::EventInput,
    pub note_off: crate::EventInput,
    voice_outputs: [crate::EventOutput; MAX_VOICES],
}

impl<const NUM_VOICES: usize> VoiceAllocatorEndpoints<NUM_VOICES> {
    pub fn voice(&self, index: usize) -> crate::EventOutput {
        assert!(
            index < NUM_VOICES,
            "Voice index {} out of range (max: {})",
            index,
            NUM_VOICES
        );
        self.voice_outputs[index]
    }

    /// Broadcast marker for use in graph! macro
    /// This method is recognized by the macro to expand broadcasting patterns
    /// Example: `voice_allocator.voices() -> voice_handlers.note_on()`
    /// expands to: `voice_allocator.voice(0) -> voice_handlers[0].note_on()`, etc.
    #[allow(unused)]
    pub fn voices(&self) -> () {
        // This is just a marker method for the macro - never called at runtime
    }

    pub fn node_key(&self) -> crate::NodeKey {
        self.node_key
    }
}

// Static descriptor array for all possible voices (up to MAX_VOICES)
const ALL_VOICE_DESCRIPTORS: [crate::graph::EndpointDescriptor; MAX_VOICES + 2] = [
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
    crate::graph::EndpointDescriptor::new(
        "voice_4",
        crate::graph::EndpointType::Event,
        crate::graph::EndpointDirection::Output,
    ),
    crate::graph::EndpointDescriptor::new(
        "voice_5",
        crate::graph::EndpointType::Event,
        crate::graph::EndpointDirection::Output,
    ),
    crate::graph::EndpointDescriptor::new(
        "voice_6",
        crate::graph::EndpointType::Event,
        crate::graph::EndpointDirection::Output,
    ),
    crate::graph::EndpointDescriptor::new(
        "voice_7",
        crate::graph::EndpointType::Event,
        crate::graph::EndpointDirection::Output,
    ),
    crate::graph::EndpointDescriptor::new(
        "voice_8",
        crate::graph::EndpointType::Event,
        crate::graph::EndpointDirection::Output,
    ),
    crate::graph::EndpointDescriptor::new(
        "voice_9",
        crate::graph::EndpointType::Event,
        crate::graph::EndpointDirection::Output,
    ),
    crate::graph::EndpointDescriptor::new(
        "voice_10",
        crate::graph::EndpointType::Event,
        crate::graph::EndpointDirection::Output,
    ),
    crate::graph::EndpointDescriptor::new(
        "voice_11",
        crate::graph::EndpointType::Event,
        crate::graph::EndpointDirection::Output,
    ),
    crate::graph::EndpointDescriptor::new(
        "voice_12",
        crate::graph::EndpointType::Event,
        crate::graph::EndpointDirection::Output,
    ),
    crate::graph::EndpointDescriptor::new(
        "voice_13",
        crate::graph::EndpointType::Event,
        crate::graph::EndpointDirection::Output,
    ),
    crate::graph::EndpointDescriptor::new(
        "voice_14",
        crate::graph::EndpointType::Event,
        crate::graph::EndpointDirection::Output,
    ),
    crate::graph::EndpointDescriptor::new(
        "voice_15",
        crate::graph::EndpointType::Event,
        crate::graph::EndpointDirection::Output,
    ),
    crate::graph::EndpointDescriptor::new(
        "voice_16",
        crate::graph::EndpointType::Event,
        crate::graph::EndpointDirection::Output,
    ),
    crate::graph::EndpointDescriptor::new(
        "voice_17",
        crate::graph::EndpointType::Event,
        crate::graph::EndpointDirection::Output,
    ),
    crate::graph::EndpointDescriptor::new(
        "voice_18",
        crate::graph::EndpointType::Event,
        crate::graph::EndpointDirection::Output,
    ),
    crate::graph::EndpointDescriptor::new(
        "voice_19",
        crate::graph::EndpointType::Event,
        crate::graph::EndpointDirection::Output,
    ),
    crate::graph::EndpointDescriptor::new(
        "voice_20",
        crate::graph::EndpointType::Event,
        crate::graph::EndpointDirection::Output,
    ),
    crate::graph::EndpointDescriptor::new(
        "voice_21",
        crate::graph::EndpointType::Event,
        crate::graph::EndpointDirection::Output,
    ),
    crate::graph::EndpointDescriptor::new(
        "voice_22",
        crate::graph::EndpointType::Event,
        crate::graph::EndpointDirection::Output,
    ),
    crate::graph::EndpointDescriptor::new(
        "voice_23",
        crate::graph::EndpointType::Event,
        crate::graph::EndpointDirection::Output,
    ),
];

// Generic implementation for any NUM_VOICES
impl<const NUM_VOICES: usize> ProcessingNode for VoiceAllocator<NUM_VOICES> {
    type Endpoints = VoiceAllocatorEndpoints<NUM_VOICES>;

    // Return all descriptors (2 inputs + MAX_VOICES outputs)
    // The graph system will only use the first NUM_VOICES + 2 descriptors
    const ENDPOINT_DESCRIPTORS: &'static [crate::graph::EndpointDescriptor] =
        &ALL_VOICE_DESCRIPTORS;

    fn create_endpoints(
        node_key: crate::NodeKey,
        inputs: arrayvec::ArrayVec<crate::ValueKey, { crate::graph::MAX_NODE_ENDPOINTS }>,
        outputs: arrayvec::ArrayVec<crate::ValueKey, { crate::graph::MAX_NODE_ENDPOINTS }>,
    ) -> Self::Endpoints {
        use crate::ValueKey;

        // Create voice outputs array - initialize with default
        let default_key = if outputs.is_empty() {
            ValueKey::default()
        } else {
            outputs[0]
        };
        let mut voice_outputs = [crate::EventOutput::new(default_key); MAX_VOICES];

        // Fill in the actual voice outputs
        for i in 0..NUM_VOICES.min(outputs.len()) {
            voice_outputs[i] = crate::EventOutput::new(outputs[i]);
        }

        VoiceAllocatorEndpoints {
            node_key,
            note_on: crate::EventInput::new(crate::graph::InputEndpoint::new(inputs[0])),
            note_off: crate::EventInput::new(crate::graph::InputEndpoint::new(inputs[1])),
            voice_outputs,
        }
    }
}

// Keep type aliases and specific endpoint types for backward compatibility
pub type VoiceAllocator2Endpoints = VoiceAllocatorEndpoints<2>;
pub type VoiceAllocator4Endpoints = VoiceAllocatorEndpoints<4>;

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
