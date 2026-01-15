use crate::graph::{
    ArrayEventOutput, EventContext, EventInput, EventInstance, EventOutput, EventPayload,
    InputEndpoint, NodeKey, ProcessingNode, SignalProcessor, ValueKey,
};
use crate::midi::{NoteOffEvent, NoteOnEvent};
use oscen_macros::Node;

const MAX_VOICES: usize = 24;

#[derive(Debug, Clone, Copy)]
struct VoiceState {
    active: bool,
    released: bool, // True if note_off received but voice may still be sounding
    note: Option<u8>,
    age: u32, // For voice stealing - higher age = older
}

impl VoiceState {
    const fn new() -> Self {
        Self {
            active: false,
            released: false,
            note: None,
            age: 0,
        }
    }
}

/// Voice allocator that distributes note events across multiple voices using CMajor-style pattern.
/// Implements LRU (least-recently-used) allocation with voice stealing when all voices are busy.
#[derive(Debug, Node)]
pub struct VoiceAllocator<const NUM_VOICES: usize> {
    #[input(event)]
    pub note_on: EventInput,

    #[input(event)]
    pub note_off: EventInput,

    #[output(event)]
    pub voices: [EventOutput; NUM_VOICES],

    // Internal state
    voice_state: [VoiceState; MAX_VOICES],
    current_age: u32,
    #[allow(dead_code)]
    sample_rate: f32,
}

impl<const NUM_VOICES: usize> VoiceAllocator<NUM_VOICES> {
    pub fn new() -> Self {
        assert!(NUM_VOICES <= MAX_VOICES, "NUM_VOICES must be <= MAX_VOICES");
        Self {
            note_on: EventInput::default(),
            note_off: EventInput::default(),
            voices: [EventOutput::default(); NUM_VOICES],
            voice_state: [VoiceState::new(); MAX_VOICES],
            current_age: 0,
            sample_rate: 44100.0, // Will be set via init()
        }
    }

    /// Find a voice to allocate for a new note (CMajor: findOldestIndex + start logic)
    fn allocate_voice(&mut self, note: u8) -> usize {
        // First, try to find an inactive voice
        for i in 0..NUM_VOICES {
            if !self.voice_state[i].active {
                self.voice_state[i].active = true;
                self.voice_state[i].released = false;
                self.voice_state[i].note = Some(note);
                self.voice_state[i].age = self.current_age;
                self.current_age += 1;
                return i;
            }
        }

        // All voices active - prefer released voices over held voices
        // Among voices of the same release state, prefer oldest (CMajor: LRU algorithm)
        let stolen_voice = (0..NUM_VOICES)
            .min_by_key(|&i| {
                let voice = &self.voice_state[i];
                // Priority: released voices first (0), then held voices (1)
                // Within each group, prefer older voices (lower age)
                let release_priority = if voice.released { 0 } else { 1 };
                (release_priority, voice.age)
            })
            .unwrap_or(0);

        self.voice_state[stolen_voice].active = true;
        self.voice_state[stolen_voice].released = false;
        self.voice_state[stolen_voice].note = Some(note);
        self.voice_state[stolen_voice].age = self.current_age;
        self.current_age += 1;

        stolen_voice
    }

    /// Find which voice is playing a specific note (not yet released)
    fn find_voice_for_note(&self, note: u8) -> Option<usize> {
        (0..NUM_VOICES).find(|&i| {
            self.voice_state[i].active
                && !self.voice_state[i].released
                && self.voice_state[i].note == Some(note)
        })
    }

    /// Release a voice (mark as released but keep active for release phase)
    fn release_voice(&mut self, voice_idx: usize) {
        if voice_idx < NUM_VOICES {
            self.voice_state[voice_idx].released = true;
            self.voice_state[voice_idx].note = None; // Clear note to prevent duplicate note_offs
                                                     // Keep active = true so the voice continues processing its release
                                                     // It will be marked inactive when stolen or reused
        }
    }

    // CMajor-style event handlers (called by Node derive macro)

    fn on_note_on(&mut self, event: &EventInstance, ctx: &mut impl EventContext) {
        if let EventPayload::Object(obj) = &event.payload {
            if let Some(note_on) = obj.as_any().downcast_ref::<NoteOnEvent>() {
                let voice_idx = self.allocate_voice(note_on.note);
                // Forward the event to the allocated voice (CMajor: voiceEventOut[oldest] <- noteOn)
                ctx.emit_event_to_array(0, voice_idx, event.clone());
            }
        }
    }

    fn on_note_off(&mut self, event: &EventInstance, ctx: &mut impl EventContext) {
        if let EventPayload::Object(obj) = &event.payload {
            if let Some(note_off) = obj.as_any().downcast_ref::<NoteOffEvent>() {
                if let Some(voice_idx) = self.find_voice_for_note(note_off.note) {
                    // Forward the event to the voice (CMajor: voiceEventOut[i] <- noteOff)
                    ctx.emit_event_to_array(0, voice_idx, event.clone());
                    self.release_voice(voice_idx);
                }
            }
        }
    }
}

impl<const NUM_VOICES: usize> Default for VoiceAllocator<NUM_VOICES> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const NUM_VOICES: usize> SignalProcessor for VoiceAllocator<NUM_VOICES> {
    fn init(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
    }

    fn process(&mut self) {
        // Event processing is handled by on_note_on() and on_note_off() event handlers
        // This node has no stream outputs to update
    }
}

// Implement ArrayEventOutput for static graph runtime multiplexing
impl<const NUM_VOICES: usize> ArrayEventOutput for VoiceAllocator<NUM_VOICES> {
    fn route_event(&mut self, input_index: usize, event: &EventInstance) -> Option<usize> {
        match input_index {
            // Input 0: note_on events
            0 => {
                if let EventPayload::Object(obj) = &event.payload {
                    if let Some(note_on) = obj.as_any().downcast_ref::<NoteOnEvent>() {
                        let voice_idx = self.allocate_voice(note_on.note);
                        return Some(voice_idx);
                    }
                }
                None
            }
            // Input 1: note_off events
            1 => {
                if let EventPayload::Object(obj) = &event.payload {
                    if let Some(note_off) = obj.as_any().downcast_ref::<NoteOffEvent>() {
                        if let Some(voice_idx) = self.find_voice_for_note(note_off.note) {
                            self.release_voice(voice_idx);
                            return Some(voice_idx);
                        }
                    }
                }
                None
            }
            _ => None,
        }
    }
}

// Note: DynNode implementation is auto-generated by #[derive(Node)] macro
// It delegates to the ArrayEventOutput implementation above

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
        assert!(allocator.voice_state[0].active);
        assert!(allocator.voice_state[1].active);
        assert!(allocator.voice_state[2].active);
        assert!(allocator.voice_state[3].active);
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
        assert_eq!(allocator.voice_state[0].note, Some(76));
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
        assert!(allocator.voice_state[1].active); // Still active (in release phase)
        assert!(allocator.voice_state[1].released); // But marked as released

        // Should not be found anymore (note is cleared)
        assert_eq!(allocator.find_voice_for_note(64), None);
    }

    #[test]
    fn test_prefer_released_voices_for_stealing() {
        let mut allocator = VoiceAllocator::<4>::new();

        // Allocate 4 voices
        allocator.allocate_voice(60);
        allocator.allocate_voice(64);
        allocator.allocate_voice(67);
        allocator.allocate_voice(72);

        // Release voice 1
        allocator.release_voice(1);

        // Allocate a 5th note - should steal voice 1 (released) instead of voice 0 (held)
        let stolen_voice = allocator.allocate_voice(76);
        assert_eq!(stolen_voice, 1);
        assert_eq!(allocator.voice_state[1].note, Some(76));
        assert!(!allocator.voice_state[1].released); // Reset on allocation
    }

    #[test]
    fn test_releasing_voice_continues_to_sound() {
        let mut allocator = VoiceAllocator::<2>::new();

        // Play first note
        let voice0 = allocator.allocate_voice(60);
        assert_eq!(voice0, 0);
        assert!(allocator.voice_state[0].active);
        assert!(!allocator.voice_state[0].released);

        // Release first note (it should continue in release phase)
        allocator.release_voice(0);
        assert!(allocator.voice_state[0].active); // Still active!
        assert!(allocator.voice_state[0].released); // But marked as released

        // Play second note - should use voice 1, not steal voice 0
        let voice1 = allocator.allocate_voice(64);
        assert_eq!(voice1, 1);
        assert!(allocator.voice_state[0].active); // Voice 0 still in release
        assert!(allocator.voice_state[1].active); // Voice 1 now playing

        // Play third note while first is releasing - NOW it should steal voice 0
        let voice2 = allocator.allocate_voice(67);
        assert_eq!(voice2, 0); // Steals the released voice, not the held one
    }
}
