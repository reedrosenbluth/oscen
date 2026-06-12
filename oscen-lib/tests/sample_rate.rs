#![feature(inherent_associated_types)]

use oscen::{Node, SampleRate, SignalProcessor};

#[derive(Debug, Node)]
struct RateNode {
    sample_rate: SampleRate,
    #[output(stream)]
    pub out: f32,
}

impl RateNode {
    fn new() -> Self {
        Self {
            sample_rate: SampleRate::default(),
            out: Default::default(),
        }
    }
}

impl SignalProcessor for RateNode {
    fn process(&mut self) {
        self.out = *self.sample_rate;
    }
}

#[derive(Debug, Node)]
struct PlainNode {
    #[output(stream)]
    pub out: f32,
}

impl PlainNode {
    fn new() -> Self {
        Self {
            out: Default::default(),
        }
    }
}

impl SignalProcessor for PlainNode {
    fn process(&mut self) {}
}

#[test]
fn set_sample_rate_fills_the_field() {
    let mut n = RateNode::new();
    n.set_sample_rate(48_000.0);
    assert_eq!(*n.sample_rate, 48_000.0);
}

#[test]
fn set_sample_rate_is_a_noop_when_absent() {
    let mut n = PlainNode::new();
    n.set_sample_rate(48_000.0); // must compile and do nothing
    let _ = &n;
}
