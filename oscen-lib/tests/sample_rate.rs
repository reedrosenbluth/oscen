#![feature(inherent_associated_types)]

use oscen::{Node, SampleRate, SignalProcessor, StreamOutput};

#[derive(Debug, Node)]
struct RateNode {
    sample_rate: SampleRate,
    pub out: StreamOutput,
}

impl RateNode {
    fn new() -> Self {
        Self {
            sample_rate: SampleRate::default(),
            out: StreamOutput::default(),
        }
    }
}

impl SignalProcessor for RateNode {
    fn process(&mut self) {
        *self.out = *self.sample_rate;
    }
}

#[derive(Debug, Node)]
struct PlainNode {
    pub out: StreamOutput,
}

impl PlainNode {
    fn new() -> Self {
        Self {
            out: StreamOutput::default(),
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
