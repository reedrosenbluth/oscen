#![feature(inherent_associated_types)]

use oscen::{graph, Node, SignalProcessor};

// Pure node-to-node wiring: the macro cannot know that `sink.input` is a
// value endpoint, so it must defer to rustc through the node's EndpointAt
// marker instead of silently summing the two drivers.

#[derive(Debug, Node)]
pub struct Src {
    #[output(stream)]
    pub output: f32,
}

impl Src {
    pub fn new() -> Self {
        Self { output: 0.0 }
    }
}

impl SignalProcessor for Src {
    fn process(&mut self) {}
}

#[derive(Debug, Node)]
pub struct ValSink {
    #[input(value)]
    pub input: f32,
    #[output(stream)]
    pub output: f32,
}

impl ValSink {
    pub fn new() -> Self {
        Self {
            input: 0.0,
            output: 0.0,
        }
    }
}

impl SignalProcessor for ValSink {
    fn process(&mut self) {
        self.output = self.input;
    }
}

graph! {
    name: ValueFanin;
    output stream out;
    nodes {
        a = Src::new();
        b = Src::new();
        sink = ValSink::new();
    }
    connections {
        a.output -> sink.input;
        b.output -> sink.input;
        sink.output -> out;
    }
}

fn main() {}
