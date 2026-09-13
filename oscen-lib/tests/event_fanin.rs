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

// ---------------------------------------------------------------------------
// Anchored (known-kind) event fan-in. Two graph event inputs feeding one node
// event input used to take the per-edge `connect` path, and every `connect`
// clears the destination first, so only the last source arrived.
// ---------------------------------------------------------------------------

graph! {
    name: AnchoredEventFanin;
    input a: event;
    input b: event;
    node sink = EvtSink::new();
    connections {
        a -> sink.ev;
        b -> sink.ev;
    }
}

#[test]
fn graph_event_inputs_fan_in_deliver_both() {
    let mut g = AnchoredEventFanin::new();
    g.init(48_000.0);
    assert!(g.push_a(1.0f32, 0));
    assert!(g.push_b(2.0f32, 0));
    g.process();
    assert_eq!(g.sink.received_a, 1, "event from `a` must arrive");
    assert_eq!(g.sink.received_b, 1, "event from `b` must arrive");
}

#[test]
fn graph_event_inputs_fan_in_agree_between_sample_and_block_paths() {
    let mut g = AnchoredEventFanin::new();
    g.init(48_000.0);
    assert!(g.push_a(1.0f32, 1));
    assert!(g.push_b(2.0f32, 1));
    assert!(g.push_a(1.0f32, 3));
    g.process_block(4);
    assert_eq!((g.sink.received_a, g.sink.received_b), (2, 1));
}

// ---------------------------------------------------------------------------
// Node event outputs merging into a graph event output.
// ---------------------------------------------------------------------------

graph! {
    name: EventOutputFanin;
    output merged: event;
    nodes {
        a = EvtSrc::new(1.0);
        b = EvtSrc::new(2.0);
    }
    connections {
        a.ev -> merged;
        b.ev -> merged;
    }
}

#[test]
fn node_event_outputs_fan_in_to_graph_event_output() {
    let mut g = EventOutputFanin::new();
    g.init(48_000.0);
    g.process();
    let tags: Vec<Option<f32>> = g.merged.iter().map(|e| e.payload.as_scalar()).collect();
    assert_eq!(tags, vec![Some(1.0), Some(2.0)], "both sources merge, in order");
    // Block path collects every frame's merged events.
    g.process_block(3);
    assert_eq!(g.merged_block.len(), 6);
}
