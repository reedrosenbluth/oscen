//! Tests for the generated `push_<event_input>()` helpers and the
//! allocation-free `Into<EventPayload>` conversions. These are the supported
//! way to inject events from a host/audio callback; the `[u8; 3]` and `f32`
//! conversions use `EventPayload::Midi` / `EventPayload::Scalar` so no heap
//! allocation happens on the audio thread.
#![feature(inherent_associated_types)]

use oscen::graph::{EventInput, EventInstance, EventPayload};
use oscen::{graph, Node, SignalProcessor};

/// Records the payload shape and frame offset of every event it receives.
#[derive(Debug, Default, Node)]
pub struct Recorder {
    #[input(event)]
    pub ev: EventInput,
    pub midi_bytes: Option<[u8; 3]>,
    pub scalar: Option<f32>,
    pub last_offset: u32,
    pub count: u32,
}

impl Recorder {
    pub fn new() -> Self {
        Self::default()
    }

    fn on_ev(&mut self, event: &EventInstance) {
        self.count += 1;
        self.last_offset = event.frame_offset;
        if let Some(bytes) = event.payload.as_midi() {
            self.midi_bytes = Some(bytes);
        }
        if let Some(v) = event.payload.as_scalar() {
            self.scalar = Some(v);
        }
    }
}

impl SignalProcessor for Recorder {
    fn process(&mut self) {}
}

graph! {
    name: PushGraph;

    input midi_in: event;

    nodes {
        rec = Recorder::new();
    }

    connections {
        midi_in -> rec.ev;
    }
}

#[test]
fn push_midi_bytes_is_allocation_free_representation() {
    let mut g = PushGraph::new();
    g.init(48_000.0);

    // Raw bytes convert to EventPayload::Midi (no Arc, no allocation).
    assert!(g.push_midi_in([0x90, 60, 100], 5));
    g.process_block(16);

    assert_eq!(g.rec.count, 1);
    assert_eq!(g.rec.midi_bytes, Some([0x90, 60, 100]));
    assert_eq!(g.rec.last_offset, 5);
}

#[test]
fn push_scalar_payload() {
    let mut g = PushGraph::new();
    g.init(48_000.0);

    assert!(g.push_midi_in(0.75f32, 0));
    g.process_block(8);

    assert_eq!(g.rec.count, 1);
    assert_eq!(g.rec.scalar, Some(0.75));
}

#[test]
fn push_explicit_payload_still_works() {
    let mut g = PushGraph::new();
    g.init(48_000.0);

    assert!(g.push_midi_in(EventPayload::Midi([0x80, 60, 0]), 2));
    g.process_block(8);

    assert_eq!(g.rec.midi_bytes, Some([0x80, 60, 0]));
}

#[test]
fn push_reports_queue_overflow() {
    let mut g = PushGraph::new();
    g.init(48_000.0);

    let capacity = oscen::graph::MAX_STATIC_EVENTS_PER_ENDPOINT;
    for i in 0..capacity {
        assert!(g.push_midi_in(i as f32, 0), "push {i} should fit");
    }
    // One past capacity: dropped, reported via `false`.
    assert!(!g.push_midi_in(0.0f32, 0));
}
