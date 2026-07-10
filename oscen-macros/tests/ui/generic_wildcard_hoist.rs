#![feature(inherent_associated_types)]

use oscen::{graph, Node, SignalProcessor};

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub enum Mode {
    #[default]
    Lowpass,
    Highpass,
}
impl oscen::graph::ValuePayload for Mode {}

// A generic node: its `mode` endpoint's declared type is the generic
// parameter `T`, which the endpoint manifest cannot resolve.
#[derive(Debug, Node)]
pub struct GenHolder<T: oscen::graph::ValuePayload + std::fmt::Debug> {
    #[input]
    pub setting: T,
    #[output(stream)]
    pub out: f32,
}
impl<T: oscen::graph::ValuePayload + std::fmt::Debug> GenHolder<T> {
    pub fn new() -> Self {
        Self {
            setting: T::default(),
            out: 0.0,
        }
    }
}
impl<T: oscen::graph::ValuePayload + std::fmt::Debug> SignalProcessor for GenHolder<T> {
    fn process(&mut self) {}
}

graph! {
    name: GenericWild;
    output stream out;
    nodes {
        holder = GenHolder::<Mode>::new();
    }
    // Wildcard-hoisting `setting` would emit an unresolved `T`: rejected
    // with a targeted diagnostic instead.
    input holder.*;
    connections {
        holder.out -> out;
    }
}

fn main() {}
