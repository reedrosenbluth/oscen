//! Feedback-edge timing. `src -> [N] -> dst` delays by exactly `N` samples,
//! and a route through a declared node (`src -> [d] -> dst`) by `d`'s own
//! latency plus one. Both hold however the scheduler orders the nodes:
//! a feedback edge reads its source as of the start of the frame.
#![feature(inherent_associated_types)]

use oscen::prelude::*;
use oscen::Node;

#[derive(Debug, Default, Node)]
pub struct Pass {
    #[input(stream)]
    pub input: f32,
    #[output(stream)]
    pub output: f32,
}

impl SignalProcessor for Pass {
    fn process(&mut self) {
        self.output = self.input;
    }
}

#[derive(Debug, Default, Node)]
pub struct Add2 {
    #[input(stream)]
    pub in1: f32,
    #[input(stream)]
    pub in2: f32,
    #[output(stream)]
    pub output: f32,
}

impl SignalProcessor for Add2 {
    fn process(&mut self) {
        self.output = self.in1 + self.in2;
    }
}

/// Feed `input` one sample per `process()` and collect the outputs.
macro_rules! run {
    ($graph:expr, $input:expr) => {{
        let mut g = $graph;
        g.init(48_000.0);
        $input
            .iter()
            .map(|&v| {
                g.x = v;
                g.process();
                g.y
            })
            .collect::<Vec<f32>>()
    }};
}

fn impulse(len: usize) -> Vec<f32> {
    (0..len).map(|t| if t == 0 { 1.0 } else { 0.0 }).collect()
}

fn latency(out: &[f32]) -> usize {
    let hits: Vec<usize> = (0..out.len()).filter(|&t| out[t] != 0.0).collect();
    assert_eq!(hits.len(), 1, "expected a single delayed impulse: {out:?}");
    assert_eq!(out[hits[0]], 1.0);
    hits[0]
}

graph! {
    name: OneIntoOutput;
    input stream x; output stream y;
    nodes { p = Pass::default(); }
    connections { x -> p.input; p.output -> [1] -> y; }
}

graph! {
    name: ThreeIntoOutput;
    input stream x; output stream y;
    nodes { p = Pass::default(); }
    connections { x -> p.input; p.output -> [3] -> y; }
}

#[test]
fn inline_delay_into_a_graph_output_is_exact() {
    assert_eq!(latency(&run!(OneIntoOutput::new(), impulse(8))), 1);
    assert_eq!(latency(&run!(ThreeIntoOutput::new(), impulse(8))), 3);
}

// The consumer `b` has no other inputs, so the scheduler runs it before the
// delay node.
graph! {
    name: InlineConsumerFirst;
    input stream x; output stream y;
    nodes { a = Pass::default(); b = Pass::default(); }
    connections { x -> a.input; a.output -> [2] -> b.input; b.output -> y; }
}

// An unrelated chain into `b` makes the scheduler run the delay node first.
graph! {
    name: InlineDelayFirst;
    input stream x; output stream y;
    nodes { a = Pass::default(); b = Add2::default(); z = Pass::default(); k = Pass::default(); }
    connections {
        x -> a.input;
        a.output -> [2] -> b.in1;
        z.output -> k.input;
        k.output -> b.in2;
        b.output -> y;
    }
}

#[test]
fn inline_delay_is_exact_whatever_the_schedule() {
    assert_eq!(latency(&run!(InlineConsumerFirst::new(), impulse(8))), 2);
    assert_eq!(latency(&run!(InlineDelayFirst::new(), impulse(8))), 2);
}

graph! {
    name: RouteConsumerFirst;
    input stream x; output stream y;
    nodes { a = Pass::default(); d = Delay::new(1.0, 0.0); b = Pass::default(); }
    connections { x -> a.input; a.output -> [d] -> b.input; b.output -> y; }
}

graph! {
    name: RouteDelayFirst;
    input stream x; output stream y;
    nodes {
        a = Pass::default(); d = Delay::new(1.0, 0.0); b = Add2::default();
        z = Pass::default(); k = Pass::default();
    }
    connections {
        x -> a.input;
        a.output -> [d] -> b.in1;
        z.output -> k.input;
        k.output -> b.in2;
        b.output -> y;
    }
}

/// `Delay::new(1.0, _)` has a latency of two samples (it reads before it
/// writes); the route adds one.
#[test]
fn declared_route_adds_one_sample_whatever_the_schedule() {
    assert_eq!(latency(&run!(RouteConsumerFirst::new(), impulse(8))), 3);
    assert_eq!(latency(&run!(RouteDelayFirst::new(), impulse(8))), 3);
}

// y[t] = x[t] + y[t - N]
graph! {
    name: Comb1;
    input stream x; output stream y;
    nodes { m = Add2::default(); }
    connections { x -> m.in1; m.output -> [1] -> m.in2; m.output -> y; }
}

graph! {
    name: Comb3;
    input stream x; output stream y;
    nodes { m = Add2::default(); }
    connections { x -> m.in1; m.output -> [3] -> m.in2; m.output -> y; }
}

#[test]
fn feedback_loop_recirculates_after_exactly_n_samples() {
    assert_eq!(run!(Comb1::new(), impulse(5)), [1.0; 5]);
    assert_eq!(
        run!(Comb3::new(), impulse(8)),
        [1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0]
    );
}

// A feedback driver and a direct driver summed into one stream input.
graph! {
    name: FanInWithFeedback;
    input stream x; output stream y;
    nodes { p = Pass::default(); s = Pass::default(); }
    connections {
        x -> p.input;
        p.output -> [1] -> s.input;
        x -> s.input;
        s.output -> y;
    }
}

#[test]
fn feedback_driver_sums_with_direct_driver() {
    let input = [1.0, 2.0, 4.0, 8.0, 0.0];
    assert_eq!(
        run!(FanInWithFeedback::new(), input),
        [1.0, 3.0, 6.0, 12.0, 8.0]
    );
}

#[test]
fn block_processing_matches_per_sample() {
    let input: Vec<f32> = (0..16).map(|t| ((t * 5) % 7) as f32).collect();
    let per_sample = run!(Comb3::new(), input);

    let mut g = Comb3::new();
    g.init(48_000.0);
    g.x_block[..16].copy_from_slice(&input);
    g.process_block(16);
    assert_eq!(g.y_block[..16], per_sample[..]);
}

// An inner-rate (2x) loop: the inline delay runs at the loop's rate and the
// feedback edge latches once per inner tick.
graph! {
    name: OversampledComb;
    input stream x; output stream y;
    nodes { m = Add2::default() * 2; }
    connections { x -> m.in1; m.output -> [1] -> m.in2; m.output -> y; }
}

#[test]
fn oversampled_feedback_loop_accumulates() {
    let out = run!(OversampledComb::new(), vec![1.0; 64]);
    assert!(out.iter().all(|v| v.is_finite()));
    // Two inner ticks per outer sample: the accumulator grows by ~2/sample.
    let slope = (out[63] - out[47]) / 16.0;
    assert!((slope - 2.0).abs() < 1e-3, "slope {slope}, out {out:?}");
}
