//! Verifies the static `AllowsFeedback` bound emitted by `graph!` for every
//! `-> [name] ->` inline-delay edge. A user-defined node referenced via the
//! bracket form that does NOT impl `oscen::graph::AllowsFeedback` must fail
//! to compile.

#![feature(inherent_associated_types)]

use oscen::graph::{StreamInput, StreamOutput};
use oscen::{graph, Node, SignalProcessor};

// User-defined `Delay` that's missing `impl AllowsFeedback for Delay`.
// The `-> [d] ->` edge below tells the macro to route through this node as
// the cycle-breaker; because no `AllowsFeedback` impl exists, the emitted
// static bound fails.
#[derive(Debug, Node)]
pub struct Delay {
    pub input: StreamInput,
    pub output: StreamOutput,
}
impl Delay {
    pub fn new() -> Self {
        Self {
            input: StreamInput::default(),
            output: StreamOutput::default(),
        }
    }
}
impl SignalProcessor for Delay {
    fn process(&mut self) {}
}

graph! {
    name: BadDelay;
    input stream src;
    output stream out;
    node g = oscen::filters::tpt::TptFilter::new(1000.0, 0.7);
    node d = Delay::new();
    connections {
        src -> g.input;
        g.output -> [d] -> g.input;
        g.output -> out;
    }
}

fn main() {}
