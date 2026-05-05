use oscen::graph::{EventOutput, StreamInput};
use oscen::{graph, Node, SignalProcessor};

#[derive(Debug, Node)]
pub struct EventEmitter {
    pub gate: EventOutput,
}
impl EventEmitter {
    pub fn new() -> Self {
        Self {
            gate: EventOutput::default(),
        }
    }
}
impl SignalProcessor for EventEmitter {
    fn process(&mut self) {}
}

#[derive(Debug, Node)]
pub struct StreamSink {
    pub input: StreamInput,
}
impl StreamSink {
    pub fn new() -> Self {
        Self {
            input: StreamInput::default(),
        }
    }
}
impl SignalProcessor for StreamSink {
    fn process(&mut self) {}
}

graph! {
    name: KindMismatch;
    nodes {
        ee = EventEmitter::new();
        ss = StreamSink::new() * 4;  // cross-rate edge below
    }
    connections {
        ee.gate -> ss.input;  // EventOutput -> StreamInput across rates: should not compile.
    }
}

fn main() {}
