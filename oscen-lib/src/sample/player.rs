//! A sample-playback node: reads a shared [`SampleBuffer`] at a variable rate,
//! gated by trigger events, with realtime-swappable source data.

use std::sync::Arc;

use crate::graph::types::EventPayload;
use crate::graph::{EventInput, EventInstance, SampleRate, SignalProcessor};
use crate::Node;

use super::buffer::SampleBuffer;
use super::slot::SampleSlot;

/// Plays back a [`SampleBuffer`] held in a realtime-swappable [`SampleSlot`].
///
/// The buffer can be replaced at any time from the control thread (via the
/// slot or the global [`SampleBank`](super::SampleBank)) and the player picks up
/// the change without glitching or allocating. Output is mono: multi-channel
/// buffers are mixed down to mono. Playback pitch is corrected for any mismatch
/// between the buffer's source rate and the graph's sample rate, so `rate = 1.0`
/// always plays at the original pitch.
///
/// # Endpoints
/// - `rate` (value): playback speed multiplier. `1.0` = original pitch, `2.0` =
///   octave up, negative = reverse.
/// - `gain` (value): output gain.
/// - `trigger` (event): scalar `> 0.5` starts playback from the beginning;
///   `<= 0.5` stops it.
/// - `output` (stream): the mono sample stream.
#[derive(Debug, Node)]
pub struct SamplePlayer {
    #[input(value)]
    pub rate: f32,

    #[input(value)]
    pub gain: f32,

    #[input(event)]
    pub trigger: EventInput,

    #[output(stream)]
    pub output: f32,

    /// Shared, swappable source data.
    slot: SampleSlot<SampleBuffer>,
    /// Audio-thread-local snapshot of the slot, refreshed only when the slot's
    /// generation changes.
    current: Option<Arc<SampleBuffer>>,
    last_generation: usize,

    /// Fractional read position, in source frames. `f64` so long samples keep
    /// sub-sample precision.
    position: f64,
    playing: bool,
    looping: bool,

    sample_rate: SampleRate,
}

impl SamplePlayer {
    /// A player not yet attached to any data. Wire it to a slot with
    /// [`with_slot`](Self::with_slot) or use [`from_buffer`](Self::from_buffer).
    pub fn new() -> Self {
        Self::with_slot(SampleSlot::empty())
    }

    /// A player reading the named buffer from the process-global
    /// [`SampleBank`](super::global_bank). The name is resolved lazily, so the
    /// data can be loaded before or after the graph is built. The string
    /// literal keeps this usable inside the `graph!` macro.
    pub fn from_buffer(name: &str) -> Self {
        Self::with_slot(super::global_bank().slot(name))
    }

    /// A player reading from an explicit slot (e.g. one shared with other
    /// players or created outside the global bank).
    pub fn with_slot(slot: SampleSlot<SampleBuffer>) -> Self {
        Self {
            rate: 1.0,
            gain: 1.0,
            trigger: EventInput::default(),
            output: 0.0,
            slot,
            current: None,
            last_generation: usize::MAX,
            position: 0.0,
            playing: false,
            looping: false,
            sample_rate: SampleRate::default(),
        }
    }

    /// Builder: loop playback instead of stopping at the end.
    pub fn looping(mut self, looping: bool) -> Self {
        self.looping = looping;
        self
    }

    /// Builder: start playing immediately (e.g. for a looping bed that doesn't
    /// need a trigger).
    pub fn playing(mut self, playing: bool) -> Self {
        self.playing = playing;
        self
    }

    /// The slot this player reads from — clone it to share the same data with
    /// other players, or to swap the data from the control thread.
    pub fn slot(&self) -> SampleSlot<SampleBuffer> {
        self.slot.clone()
    }

    /// Pull a fresh buffer snapshot if the slot changed. Realtime-safe: a cheap
    /// generation check, and a non-blocking `try_load` only when it differs.
    #[inline]
    fn refresh(&mut self) {
        let generation = self.slot.generation();
        if generation != self.last_generation {
            if let Some(snapshot) = self.slot.try_load() {
                self.current = snapshot;
                self.last_generation = generation;
            }
        }
    }

    fn on_trigger(&mut self, event: &EventInstance) {
        let value = match &event.payload {
            EventPayload::Scalar(v) => *v,
            EventPayload::Object(_) => 1.0,
        };
        if value > 0.5 {
            self.playing = true;
            // Reverse playback starts from the tail.
            self.position = if self.rate < 0.0 {
                self.current
                    .as_ref()
                    .map(|b| (b.frames() as f64 - 1.0).max(0.0))
                    .unwrap_or(0.0)
            } else {
                0.0
            };
        } else {
            self.playing = false;
        }
    }
}

impl Default for SamplePlayer {
    fn default() -> Self {
        Self::new()
    }
}

impl SignalProcessor for SamplePlayer {
    #[inline]
    fn process(&mut self) {
        self.refresh();

        if !self.playing {
            self.output = 0.0;
            return;
        }

        let Some(buffer) = self.current.as_ref() else {
            self.output = 0.0;
            return;
        };
        let frames = buffer.frames();
        if frames == 0 {
            self.output = 0.0;
            self.playing = false;
            return;
        }

        self.output = buffer.read_mono_linear(self.position) * self.gain;

        // Advance, correcting for source-vs-graph sample-rate mismatch.
        let graph_rate = (*self.sample_rate).max(f32::EPSILON) as f64;
        let increment = self.rate as f64 * buffer.source_rate() as f64 / graph_rate;
        self.position += increment;

        let len = frames as f64;
        if self.looping {
            if self.position >= len {
                self.position -= len;
            } else if self.position < 0.0 {
                self.position += len;
            }
        } else if self.position >= len || self.position < 0.0 {
            // Played through the final frame (the read clamps to the last
            // sample for fractional positions in the last interval).
            self.playing = false;
        }
    }

    #[inline]
    fn is_active(&self) -> bool {
        self.playing
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::types::EventPayload;

    fn gate_on() -> EventInstance {
        EventInstance {
            frame_offset: 0,
            payload: EventPayload::scalar(1.0),
        }
    }

    fn make_player(buf: SampleBuffer) -> SamplePlayer {
        let slot = SampleSlot::new(Arc::new(buf));
        let mut player = SamplePlayer::with_slot(slot);
        player.set_sample_rate(44100.0);
        player
    }

    #[test]
    fn silent_until_triggered() {
        let mut player = make_player(SampleBuffer::from_planar(vec![1.0, 1.0, 1.0], 1, 44100.0));
        player.process();
        assert_eq!(player.output, 0.0);
        assert!(!player.is_active());
    }

    #[test]
    fn plays_samples_after_trigger() {
        let mut player =
            make_player(SampleBuffer::from_planar(vec![0.1, 0.2, 0.3, 0.4], 1, 44100.0));
        player.on_trigger(&gate_on());
        assert!(player.is_active());
        player.process();
        assert!((player.output - 0.1).abs() < 1e-6);
        player.process();
        assert!((player.output - 0.2).abs() < 1e-6);
    }

    #[test]
    fn one_shot_stops_at_end() {
        let mut player = make_player(SampleBuffer::from_planar(vec![0.5, 0.5], 1, 44100.0));
        player.on_trigger(&gate_on());
        for _ in 0..8 {
            player.process();
        }
        assert!(!player.is_active());
        assert_eq!(player.output, 0.0);
    }

    #[test]
    fn looping_wraps_and_keeps_playing() {
        let mut player =
            SamplePlayer::with_slot(SampleSlot::new(Arc::new(SampleBuffer::from_planar(
                vec![0.5, 0.5, 0.5, 0.5],
                1,
                44100.0,
            ))))
            .looping(true);
        player.set_sample_rate(44100.0);
        player.on_trigger(&gate_on());
        for _ in 0..32 {
            player.process();
        }
        assert!(player.is_active());
    }

    #[test]
    fn gain_scales_output() {
        let mut player = make_player(SampleBuffer::from_planar(vec![1.0, 1.0], 1, 44100.0));
        player.gain = 0.5;
        player.on_trigger(&gate_on());
        player.process();
        assert!((player.output - 0.5).abs() < 1e-6);
    }

    #[test]
    fn picks_up_swapped_buffer() {
        let slot = SampleSlot::new(Arc::new(SampleBuffer::from_planar(
            vec![0.1, 0.1],
            1,
            44100.0,
        )));
        let mut player = SamplePlayer::with_slot(slot.clone());
        player.set_sample_rate(44100.0);
        player.on_trigger(&gate_on());
        player.process();
        assert!((player.output - 0.1).abs() < 1e-6);

        // Swap in new data from the "control thread".
        slot.store(Arc::new(SampleBuffer::from_planar(vec![0.9, 0.9], 1, 44100.0)));
        player.on_trigger(&gate_on()); // restart
        player.process();
        assert!((player.output - 0.9).abs() < 1e-6);
    }
}
