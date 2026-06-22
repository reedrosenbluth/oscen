//! Regression guard for auto-summing stream fan-in: an **event** fan-in (≥2
//! event sources into one event input) must keep compiling and running.
//!
//! Event endpoints in a pure node-to-node graph have an unknown kind at compile
//! time, so the fan-in lowering cannot tell them apart from stream endpoints by
//! kind alone. Event queues do not implement `Add`, so a naive `dest = a + b`
//! sum would fail to compile; the lowering instead routes the extra sources
//! through `AccumulateEndpoints`, which for events delegates to the existing
//! copy path (last-write-wins) — leaving event behavior unchanged.
#![feature(inherent_associated_types)]

use oscen::graph::{EventInput, EventInstance, EventOutput, EventPayload};
use oscen::{graph, Node, SignalProcessor};

/// Emits one event per `process()` on its event output.
#[derive(Debug, Default, Node)]
pub struct EvtSrc {
    #[output(event)]
    pub ev: EventOutput,
}

impl EvtSrc {
    pub fn new() -> Self {
        Self::default()
    }
}

impl SignalProcessor for EvtSrc {
    fn process(&mut self) {
        let _ = self.ev.try_push(EventInstance {
            frame_offset: 0,
            payload: EventPayload::scalar(1.0),
        });
    }
}

/// Counts the events delivered to its event input.
#[derive(Debug, Default, Node)]
pub struct EvtSink {
    #[input(event)]
    pub ev: EventInput,
    pub received: u32,
}

impl EvtSink {
    pub fn new() -> Self {
        Self::default()
    }

    fn on_ev(&mut self, _event: &EventInstance) {
        self.received += 1;
    }
}

impl SignalProcessor for EvtSink {
    fn process(&mut self) {}
}

graph! {
    name: EventFaninGraph;

    nodes {
        a = EvtSrc::new();
        b = EvtSrc::new();
        sink = EvtSink::new();
    }

    connections {
        a.ev -> sink.ev;
        b.ev -> sink.ev;
    }
}

#[test]
fn event_fanin_compiles_and_runs() {
    let mut graph = EventFaninGraph::new();
    graph.init(48_000.0);
    // The point of this test is that codegen compiles (events are not summed)
    // and the graph runs without panicking; event delivery is exercised too.
    for _ in 0..4 {
        graph.process();
    }
    assert!(
        graph.sink.received > 0,
        "event fan-in should still deliver events (got {})",
        graph.sink.received
    );
}
