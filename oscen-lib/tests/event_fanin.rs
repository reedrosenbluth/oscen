//! Regression test for event fan-in: with ≥2 event sources connected to one
//! event input, events from **all** sources must arrive.
//!
//! Event endpoints in a pure node-to-node graph have an unknown kind at compile
//! time, so the fan-in lowering cannot tell them apart from stream endpoints by
//! kind alone. Event queues do not implement `Add`, so a naive `dest = a + b`
//! sum would fail to compile; the lowering instead routes the extra sources
//! through `AccumulateEndpoints`, which for events appends to the destination
//! queue. Previously it delegated to `connect` (which clears the destination
//! first), so every source but the last was silently discarded.
#![feature(inherent_associated_types)]

use oscen::graph::{EventInput, EventInstance, EventOutput, EventPayload};
use oscen::{graph, Node, SignalProcessor};

/// Emits one event with a fixed scalar payload per `process()`.
#[derive(Debug, Default, Node)]
pub struct EvtSrc {
    #[output(event)]
    pub ev: EventOutput,
    pub tag: f32,
}

impl EvtSrc {
    pub fn new(tag: f32) -> Self {
        Self {
            ev: EventOutput::new(),
            tag,
        }
    }
}

impl SignalProcessor for EvtSrc {
    fn process(&mut self) {
        let _ = self.ev.try_push(EventInstance {
            frame_offset: 0,
            payload: EventPayload::scalar(self.tag),
        });
    }
}

/// Counts the events delivered to its event input, per payload tag.
#[derive(Debug, Default, Node)]
pub struct EvtSink {
    #[input(event)]
    pub ev: EventInput,
    pub received_a: u32,
    pub received_b: u32,
}

impl EvtSink {
    pub fn new() -> Self {
        Self::default()
    }

    fn on_ev(&mut self, event: &EventInstance) {
        use float_cmp::approx_eq;
        match event.payload.as_scalar() {
            Some(v) if approx_eq!(f32, v, 1.0) => self.received_a += 1,
            Some(v) if approx_eq!(f32, v, 2.0) => self.received_b += 1,
            _ => {}
        }
    }
}

impl SignalProcessor for EvtSink {
    fn process(&mut self) {}
}

graph! {
    name: EventFaninGraph;

    nodes {
        a = EvtSrc::new(1.0);
        b = EvtSrc::new(2.0);
        sink = EvtSink::new();
    }

    connections {
        a.ev -> sink.ev;
        b.ev -> sink.ev;
    }
}

#[test]
fn event_fanin_delivers_all_sources() {
    let mut graph = EventFaninGraph::new();
    graph.init(48_000.0);
    for _ in 0..4 {
        graph.process();
    }
    // Each source emits one event per frame; all of them must arrive.
    assert_eq!(
        graph.sink.received_a, 4,
        "all events from source `a` should arrive"
    );
    assert_eq!(
        graph.sink.received_b, 4,
        "all events from source `b` should arrive"
    );
}
