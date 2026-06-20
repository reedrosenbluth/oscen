//! Sample-playback nodes: read a shared [`SampleBuffer`] at a variable rate,
//! gated by trigger events, with realtime-swappable source data.
//!
//! Two flavors share one transport ([`Voice`]):
//! - [`SamplePlayer`] — `f32` mono output (the canonical mono type; connects
//!   straight to an `f32` graph output).
//! - [`SamplePlayerN`] — `Frame<N>` output for stereo/multichannel samples.

use std::sync::Arc;

use crate::frame::Frame;
use crate::graph::types::EventPayload;
use crate::graph::{EventInput, EventInstance, SampleRate, SignalProcessor};
use crate::Node;

use super::buffer::SampleBuffer;
use super::slot::SampleSlot;

/// Shared transport: holds the swappable slot, the audio-thread-local buffer
/// snapshot, and the play position. Both player nodes delegate to this so the
/// realtime-safe refresh, triggering, and looping logic lives in one place.
#[derive(Debug)]
struct Voice {
    slot: SampleSlot<SampleBuffer>,
    /// Audio-thread-local snapshot, refreshed only when the slot's generation
    /// changes.
    current: Option<Arc<SampleBuffer>>,
    last_generation: usize,
    /// Fractional read position, in source frames. `f64` so long samples keep
    /// sub-sample precision.
    position: f64,
    playing: bool,
    looping: bool,
}

impl Voice {
    fn with_slot(slot: SampleSlot<SampleBuffer>) -> Self {
        Self {
            slot,
            current: None,
            last_generation: usize::MAX,
            position: 0.0,
            playing: false,
            looping: false,
        }
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

    /// Handle a trigger event payload: `> 0.5` starts from the top (or the tail
    /// for reverse playback), `<= 0.5` stops.
    fn trigger(&mut self, value: f32, rate: f32) {
        if value > 0.5 {
            self.playing = true;
            self.position = if rate < 0.0 {
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

    /// The buffer to read this sample, or `None` if silent. A playing-but-empty
    /// buffer stops the voice.
    #[inline]
    fn active_buffer(&mut self) -> Option<&SampleBuffer> {
        if !self.playing {
            return None;
        }
        match self.current.as_ref() {
            Some(buf) if buf.frames() > 0 => Some(buf),
            Some(_) => {
                self.playing = false;
                None
            }
            None => None,
        }
    }

    /// Advance the play position after a read, applying loop/stop at the edges.
    #[inline]
    fn advance(&mut self, rate: f32, sample_rate: f32) {
        let Some(buffer) = self.current.as_ref() else {
            return;
        };
        let frames = buffer.frames();
        if frames == 0 {
            return;
        }

        // Correct for source-vs-graph sample-rate mismatch so rate 1.0 plays at
        // the original pitch.
        let graph_rate = sample_rate.max(f32::EPSILON) as f64;
        self.position += rate as f64 * buffer.source_rate() as f64 / graph_rate;

        let len = frames as f64;
        if self.looping {
            if self.position >= len {
                self.position -= len;
            } else if self.position < 0.0 {
                self.position += len;
            }
        } else if self.position >= len || self.position < 0.0 {
            self.playing = false;
        }
    }
}

/// Plays back a mono `f32` stream from a realtime-swappable [`SampleSlot`].
/// Multi-channel buffers are mixed down to mono. See the module docs for the
/// loading / swapping workflow.
///
/// # Endpoints
/// - `rate` (value): playback speed. `1.0` = original pitch, negative = reverse.
/// - `gain` (value): output gain.
/// - `trigger` (event): scalar `> 0.5` starts playback from the top; `<= 0.5` stops.
/// - `output` (stream): mono sample stream.
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

    voice: Voice,
    sample_rate: SampleRate,
}

impl SamplePlayer {
    /// A player not yet attached to any data.
    pub fn new() -> Self {
        Self::with_slot(SampleSlot::empty())
    }

    /// A player reading the named buffer from the process-global
    /// [`SampleBank`](super::global_bank). Resolved lazily; the string literal
    /// keeps this usable inside `graph!`.
    pub fn from_buffer(name: &str) -> Self {
        Self::with_slot(super::global_bank().slot(name))
    }

    /// A player reading from an explicit slot.
    pub fn with_slot(slot: SampleSlot<SampleBuffer>) -> Self {
        Self {
            rate: 1.0,
            gain: 1.0,
            trigger: EventInput::default(),
            output: 0.0,
            voice: Voice::with_slot(slot),
            sample_rate: SampleRate::default(),
        }
    }

    /// Builder: loop instead of stopping at the end.
    pub fn looping(mut self, looping: bool) -> Self {
        self.voice.looping = looping;
        self
    }

    /// Builder: start playing immediately.
    pub fn playing(mut self, playing: bool) -> Self {
        self.voice.playing = playing;
        self
    }

    /// The slot this player reads from — clone it to share data with other
    /// players or to swap the data from the control thread.
    pub fn slot(&self) -> SampleSlot<SampleBuffer> {
        self.voice.slot.clone()
    }

    fn on_trigger(&mut self, event: &EventInstance) {
        let value = match &event.payload {
            EventPayload::Scalar(v) => *v,
            EventPayload::Object(_) => 1.0,
        };
        self.voice.trigger(value, self.rate);
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
        self.voice.refresh();
        let gain = self.gain;
        let pos = self.voice.position;
        self.output = match self.voice.active_buffer() {
            Some(buf) => buf.read_mono_linear(pos) * gain,
            None => 0.0,
        };
        self.voice.advance(self.rate, *self.sample_rate);
    }

    #[inline]
    fn is_active(&self) -> bool {
        self.voice.playing
    }
}

/// Plays back an `N`-channel `Frame<N>` stream from a realtime-swappable
/// [`SampleSlot`]. Buffer channels map to frame channels positionally; a mono
/// buffer is broadcast to every channel. Use this for stereo/multichannel
/// samples; for mono prefer [`SamplePlayer`] (canonical `f32`).
///
/// Construct with turbofish in graphs: `SamplePlayerN::<2>::from_buffer("loop")`.
#[derive(Debug, Node)]
pub struct SamplePlayerN<const N: usize> {
    #[input(value)]
    pub rate: f32,
    #[input(value)]
    pub gain: f32,
    #[input(event)]
    pub trigger: EventInput,
    #[output(stream)]
    pub output: Frame<N>,

    voice: Voice,
    sample_rate: SampleRate,
}

impl<const N: usize> SamplePlayerN<N> {
    pub fn new() -> Self {
        Self::with_slot(SampleSlot::empty())
    }

    pub fn from_buffer(name: &str) -> Self {
        Self::with_slot(super::global_bank().slot(name))
    }

    pub fn with_slot(slot: SampleSlot<SampleBuffer>) -> Self {
        Self {
            rate: 1.0,
            gain: 1.0,
            trigger: EventInput::default(),
            output: Frame([0.0; N]),
            voice: Voice::with_slot(slot),
            sample_rate: SampleRate::default(),
        }
    }

    pub fn looping(mut self, looping: bool) -> Self {
        self.voice.looping = looping;
        self
    }

    pub fn playing(mut self, playing: bool) -> Self {
        self.voice.playing = playing;
        self
    }

    pub fn slot(&self) -> SampleSlot<SampleBuffer> {
        self.voice.slot.clone()
    }

    fn on_trigger(&mut self, event: &EventInstance) {
        let value = match &event.payload {
            EventPayload::Scalar(v) => *v,
            EventPayload::Object(_) => 1.0,
        };
        self.voice.trigger(value, self.rate);
    }
}

impl<const N: usize> Default for SamplePlayerN<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> SignalProcessor for SamplePlayerN<N> {
    #[inline]
    fn process(&mut self) {
        self.voice.refresh();
        let gain = self.gain;
        let pos = self.voice.position;
        self.output = match self.voice.active_buffer() {
            Some(buf) => buf.read_frame_linear::<N>(pos) * gain,
            None => Frame([0.0; N]),
        };
        self.voice.advance(self.rate, *self.sample_rate);
    }

    #[inline]
    fn is_active(&self) -> bool {
        self.voice.playing
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
        let mut player = SamplePlayer::with_slot(SampleSlot::new(Arc::new(buf)));
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
    fn one_shot_plays_through_last_sample_then_stops() {
        let mut player = make_player(SampleBuffer::from_planar(vec![0.25, 0.5], 1, 44100.0));
        player.on_trigger(&gate_on());
        player.process();
        assert!((player.output - 0.25).abs() < 1e-6);
        player.process();
        assert!((player.output - 0.5).abs() < 1e-6); // final sample is played
        player.process();
        assert!(!player.is_active());
        assert_eq!(player.output, 0.0);
    }

    #[test]
    fn looping_wraps_and_keeps_playing() {
        let mut player = SamplePlayer::with_slot(SampleSlot::new(Arc::new(
            SampleBuffer::from_planar(vec![0.5, 0.5, 0.5, 0.5], 1, 44100.0),
        )))
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

        slot.store(Arc::new(SampleBuffer::from_planar(vec![0.9, 0.9], 1, 44100.0)));
        player.on_trigger(&gate_on());
        player.process();
        assert!((player.output - 0.9).abs() < 1e-6);
    }

    #[test]
    fn multichannel_player_reads_per_channel() {
        // Stereo buffer: frame0 = (0.2, -0.2)
        let buf = SampleBuffer::from_interleaved(&[0.2, -0.2, 0.4, -0.4], 2, 44100.0);
        let mut player = SamplePlayerN::<2>::with_slot(SampleSlot::new(Arc::new(buf)));
        player.set_sample_rate(44100.0);
        player.on_trigger(&gate_on());
        player.process();
        assert_eq!(player.output, Frame([0.2, -0.2]));
        player.process();
        assert_eq!(player.output, Frame([0.4, -0.4]));
    }

    #[test]
    fn multichannel_player_broadcasts_mono() {
        let buf = SampleBuffer::from_planar(vec![0.3, 0.6], 1, 44100.0);
        let mut player = SamplePlayerN::<2>::with_slot(SampleSlot::new(Arc::new(buf)));
        player.set_sample_rate(44100.0);
        player.on_trigger(&gate_on());
        player.process();
        assert_eq!(player.output, Frame([0.3, 0.3]));
    }
}

#[cfg(test)]
mod graph_tests {
    use super::*;
    use crate::graph::types::EventPayload;
    use crate::prelude::*;
    use crate::SignalProcessor;

    // Collapses the stereo player's Frame<2> down to f32 at the graph boundary
    // (graph-level stream outputs are f32).
    #[derive(Debug, Node)]
    struct Mixdown2 {
        #[input(stream)]
        input: Frame<2>,
        #[output(stream)]
        output: f32,
    }
    impl Mixdown2 {
        fn new() -> Self {
            Self {
                input: Frame([0.0; 2]),
                output: 0.0,
            }
        }
    }
    impl SignalProcessor for Mixdown2 {
        fn process(&mut self) {
            self.output = (self.input.0[0] + self.input.0[1]) * 0.5;
        }
    }

    graph! {
        name: StereoSampleGraph;
        input trigger: event;
        output out: stream;
        nodes {
            player = SamplePlayerN::<2>::from_buffer("__stereo_graph_test");
            mix = Mixdown2::new();
        }
        connections {
            trigger -> player.trigger;
            player.output -> mix.input;
            mix.output -> out;
        }
    }

    #[test]
    fn stereo_player_wires_through_graph() {
        super::super::global_bank().store(
            "__stereo_graph_test",
            Arc::new(SampleBuffer::from_interleaved(
                &[0.2, 0.4, 0.2, 0.4],
                2,
                48_000.0,
            )),
        );

        let mut g = StereoSampleGraph::new();
        g.init(48_000.0);
        let _ = g.trigger.try_push(EventInstance {
            frame_offset: 0,
            payload: EventPayload::scalar(1.0),
        });
        g.process();
        // mix = (0.2 + 0.4) / 2 = 0.3
        assert!((g.get_stream_output(0).unwrap() - 0.3).abs() < 1e-6);
    }
}
