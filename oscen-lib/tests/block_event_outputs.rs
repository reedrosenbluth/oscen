//! Regression tests for graph event outputs under block processing.
//!
//! The per-frame `output <name>: event` queue is overwritten every frame, so
//! after `process_block` it only reflects the final frame. Every event output
//! also gets a `<name>_block` accumulator that `process_block` clears once at
//! block start and fills with every event seen at the graph boundary, with
//! `frame_offset` stamped to the block-relative frame index.
#![feature(inherent_associated_types)]

use oscen::graph::{EventInstance, EventOutput, EventPayload};
use oscen::{graph, Node, SignalProcessor};

// ---------------------------------------------------------------------------
// 1. Direct passthrough `e -> o`
// ---------------------------------------------------------------------------

graph! {
    name: Thru;
    input e: event;
    output o: event;
    connections {
        e -> o;
    }
}

fn block_offsets<'a>(q: impl Iterator<Item = &'a EventInstance>) -> Vec<u32> {
    q.map(|e| e.frame_offset).collect()
}

#[test]
fn passthrough_event_at_frame_one_survives_the_block() {
    let mut g = Thru::new();
    g.init(48_000.0);
    assert!(g.push_e(1.0f32, 1));
    g.process_block(4);
    assert_eq!(g.o_block.len(), 1, "the event must be retained after the block");
    assert_eq!(g.o_block[0].frame_offset, 1);
    assert_eq!(g.o_block[0].payload.as_scalar(), Some(1.0));
    // The per-frame queue reflects only the last frame (frame 3), which had
    // no events.
    assert!(g.o.is_empty());
}

#[test]
fn passthrough_events_keep_block_order_and_offsets() {
    let mut g = Thru::new();
    g.init(48_000.0);
    assert!(g.push_e(1.0f32, 0));
    assert!(g.push_e(2.0f32, 3));
    g.process_block(4);
    assert_eq!(block_offsets(g.o_block.iter()), vec![0, 3]);
    let payloads: Vec<_> = g.o_block.iter().map(|e| e.payload.as_scalar()).collect();
    assert_eq!(payloads, vec![Some(1.0), Some(2.0)]);
}

#[test]
fn block_accumulator_is_cleared_each_block() {
    let mut g = Thru::new();
    g.init(48_000.0);
    assert!(g.push_e(1.0f32, 2));
    g.process_block(4);
    assert_eq!(g.o_block.len(), 1);
    g.process_block(4);
    assert!(g.o_block.is_empty(), "no events in the second block");
}

#[test]
fn deferred_event_lands_in_the_later_block_with_rebased_offset() {
    let mut g = Thru::new();
    g.init(48_000.0);
    // Offset 6 is beyond a 4-frame block: it must be deferred and fire at
    // frame 2 of the next block.
    assert!(g.push_e(1.0f32, 6));
    g.process_block(4);
    assert!(g.o_block.is_empty(), "event beyond the block must not fire yet");
    g.process_block(4);
    assert_eq!(block_offsets(g.o_block.iter()), vec![2]);
}

#[test]
fn per_sample_process_keeps_last_frame_semantics() {
    // `process()` is unchanged: the per-frame queue holds this cycle's events.
    let mut g = Thru::new();
    g.init(48_000.0);
    assert!(g.push_e(1.0f32, 0));
    g.process();
    assert_eq!(g.o.len(), 1);
    g.process();
    assert!(g.o.is_empty());
}

// ---------------------------------------------------------------------------
// 2. A node that emits events on its own schedule (frame_offset: 0, like the
//    library nodes do)
// ---------------------------------------------------------------------------

#[derive(Debug, Node)]
pub struct Ticker {
    #[output(event)]
    pub tick: EventOutput,
    pub period: u32,
    pub n: u32,
}

impl Ticker {
    pub fn new(period: u32) -> Self {
        Self {
            tick: EventOutput::new(),
            period,
            n: 0,
        }
    }
}

impl SignalProcessor for Ticker {
    fn process(&mut self) {
        if self.n % self.period == 0 {
            let _ = self.tick.try_push(EventInstance {
                frame_offset: 0,
                payload: EventPayload::scalar(self.n as f32),
            });
        }
        self.n += 1;
    }
}

graph! {
    name: TickGraph;
    output t: event;
    node ticker = Ticker::new(3);
    connections {
        ticker.tick -> t;
    }
}

#[test]
fn node_emitted_events_are_stamped_with_their_frame() {
    // Reference: observe the per-frame queue with `process()`.
    let mut reference = TickGraph::new();
    reference.init(48_000.0);
    let mut expected = Vec::new();
    for frame in 0..8u32 {
        reference.process();
        for e in reference.t.iter() {
            expected.push((frame, e.payload.as_scalar()));
        }
    }
    assert_eq!(expected.len(), 3, "ticks at frames 0, 3, 6");

    let mut g = TickGraph::new();
    g.init(48_000.0);
    g.process_block(8);
    let got: Vec<_> = g
        .t_block
        .iter()
        .map(|e| (e.frame_offset, e.payload.as_scalar()))
        .collect();
    assert_eq!(got, expected);
}

// ---------------------------------------------------------------------------
// 3. Nested graph: the inner graph's event output feeds the outer output
// ---------------------------------------------------------------------------

graph! {
    name: Outer;
    input e: event;
    output o: event;
    node inner = Thru::new();
    connections {
        e -> inner.e;
        inner.o -> o;
    }
}

#[test]
fn nested_graph_event_output_is_collected_by_the_outer_block() {
    let mut g = Outer::new();
    g.init(48_000.0);
    assert!(g.push_e(1.0f32, 1));
    assert!(g.push_e(2.0f32, 5));
    g.process_block(8);
    assert_eq!(block_offsets(g.o_block.iter()), vec![1, 5]);
}

// ---------------------------------------------------------------------------
// 4. Cross-rate source: an oversampled ticker feeding a base-rate output
// ---------------------------------------------------------------------------

graph! {
    name: FastTickGraph;
    output t: event;
    node ticker = Ticker::new(3) * 2;
    connections {
        ticker.tick -> t;
    }
}

#[test]
fn cross_rate_event_output_matches_per_sample_observation() {
    let mut reference = FastTickGraph::new();
    reference.init(48_000.0);
    let mut expected = Vec::new();
    for frame in 0..8u32 {
        reference.process();
        for e in reference.t.iter() {
            expected.push((frame, e.payload.as_scalar()));
        }
    }
    assert!(!expected.is_empty(), "the oversampled ticker must emit events");

    let mut g = FastTickGraph::new();
    g.init(48_000.0);
    g.process_block(8);
    let got: Vec<_> = g
        .t_block
        .iter()
        .map(|e| (e.frame_offset, e.payload.as_scalar()))
        .collect();
    assert_eq!(got, expected);
}
