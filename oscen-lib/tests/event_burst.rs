//! Regression test for event queue capacity: a burst larger than the old
//! 32-event limit (e.g. a MIDI panic sending many note-offs in one buffer)
//! must be delivered in full, not silently truncated.
#![feature(inherent_associated_types)]

use oscen::graph::{EventInput, EventInstance, EventPayload, MAX_STATIC_EVENTS_PER_ENDPOINT};
use oscen::{graph, Node, SignalProcessor};

/// Counts the events delivered to its event input.
#[derive(Debug, Default, Node)]
pub struct Counter {
    #[input(event)]
    pub ev: EventInput,
    pub received: u32,
}

impl Counter {
    pub fn new() -> Self {
        Self::default()
    }

    fn on_ev(&mut self, _event: &EventInstance) {
        self.received += 1;
    }
}

impl SignalProcessor for Counter {
    fn process(&mut self) {}
}

graph! {
    name: BurstGraph;

    input ev_in: event;

    nodes {
        counter = Counter::new();
    }

    connections {
        ev_in -> counter.ev;
    }
}

#[test]
fn burst_of_128_events_is_delivered_in_full() {
    assert_eq!(MAX_STATIC_EVENTS_PER_ENDPOINT, 128);

    let mut graph = BurstGraph::new();
    graph.init(48_000.0);
    for _ in 0..MAX_STATIC_EVENTS_PER_ENDPOINT {
        graph
            .ev_in
            .try_push(EventInstance {
                frame_offset: 0,
                payload: EventPayload::scalar(0.0),
            })
            .expect("queue holds a full 128-event burst");
    }
    graph.process();
    assert_eq!(
        graph.counter.received, MAX_STATIC_EVENTS_PER_ENDPOINT as u32,
        "every event in the burst should be dispatched"
    );
}

/// Overflowing an event queue silently drops in release, but must be
/// observable in debug builds.
#[cfg(debug_assertions)]
#[test]
#[should_panic(expected = "static event queue overflow")]
fn dropped_event_panics_in_debug_builds() {
    use oscen::graph::{AccumulateEndpoints, EventOutput};

    let event = || EventInstance {
        frame_offset: 0,
        payload: EventPayload::scalar(0.0),
    };
    let mut src: EventOutput = EventOutput::new();
    for _ in 0..MAX_STATIC_EVENTS_PER_ENDPOINT {
        src.try_push(event()).unwrap();
    }
    let mut dst: EventInput = EventInput::new();
    dst.try_push(event()).unwrap();
    // 128 incoming events + 1 already queued: the last push must overflow.
    <() as AccumulateEndpoints<_, _>>::accumulate(&src, &mut dst);
}
