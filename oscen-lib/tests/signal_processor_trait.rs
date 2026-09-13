//! Regression test: a generated graph must do real work when driven through
//! the `SignalProcessor` trait (generic bound, UFCS, or trait object), not
//! only through its inherent `process()`.
//!
//! Previously the generated trait impl had an empty `process` body, so
//! `fn tick<T: SignalProcessor>(p: &mut T) { p.process() }` left the graph's
//! outputs untouched while the inherent method worked.
#![feature(inherent_associated_types)]

use oscen::graph::{EventInput, EventInstance};
use oscen::{graph, Gain, Node, PolyBlepOscillator, SignalProcessor};

graph! {
    name: GainGraph;
    input x: stream;
    output y: stream;
    node gain = Gain::new(2.0);
    connections {
        x -> gain.input;
        gain.output -> y;
    }
}

fn generic_tick<T: SignalProcessor>(p: &mut T) {
    p.process();
}

#[test]
fn generic_bound_processes_generated_graph() {
    let mut g = GainGraph::new();
    g.init(48_000.0);
    g.x = 3.0;
    generic_tick(&mut g);
    assert_eq!(g.y, 6.0, "generic SignalProcessor call must run the graph");
}

#[test]
fn ufcs_processes_generated_graph() {
    let mut g = GainGraph::new();
    g.init(48_000.0);
    g.x = 3.0;
    <GainGraph as SignalProcessor>::process(&mut g);
    assert_eq!(g.y, 6.0, "UFCS trait call must run the graph");
}

#[test]
fn trait_object_processes_generated_graph() {
    let mut g = GainGraph::new();
    g.init(48_000.0);
    g.x = 3.0;
    let dynp: &mut dyn SignalProcessor = &mut g;
    dynp.process();
    assert_eq!(g.y, 6.0, "trait-object call must run the graph");
}

/// Counts delivered events so we can prove the trait path runs the full
/// per-cycle wrapper (clear outputs → frame core → clear inputs), not just
/// the inner frame body.
#[derive(Debug, Default, Node)]
pub struct Counter {
    #[input(event)]
    pub ev: EventInput,
    pub seen: u32,
}

impl Counter {
    pub fn new() -> Self {
        Self::default()
    }
    fn on_ev(&mut self, _e: &EventInstance) {
        self.seen += 1;
    }
}

impl SignalProcessor for Counter {
    fn process(&mut self) {}
}

graph! {
    name: EventGraph;
    input e: event;
    node sink = Counter::new();
    connections {
        e -> sink.ev;
    }
}

#[test]
fn trait_path_runs_full_cycle_including_event_input_clearing() {
    let mut g = EventGraph::new();
    g.init(48_000.0);
    assert!(g.push_e(1.0, 0));
    generic_tick(&mut g);
    assert_eq!(g.sink.seen, 1, "event delivered through trait path");
    assert!(
        g.e.is_empty(),
        "trait path must clear event inputs after the cycle"
    );
    // A second cycle with no new events must not re-deliver the old one.
    generic_tick(&mut g);
    assert_eq!(g.sink.seen, 1, "cleared event must not be redelivered");
}

graph! {
    name: OscGraph;
    output out: stream;
    node osc = PolyBlepOscillator::saw(440.0, 0.5);
    connections {
        osc.output -> out;
    }
}

#[test]
fn trait_and_inherent_paths_produce_identical_output() {
    let mut a = OscGraph::new();
    let mut b = OscGraph::new();
    a.init(48_000.0);
    b.init(48_000.0);
    for i in 0..64 {
        a.process();
        generic_tick(&mut b);
        assert_eq!(a.out, b.out, "sample {i} differs between inherent and trait paths");
    }
}
