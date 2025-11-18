use crate::graph::{InputEndpoint, NodeKey, ProcessingNode, SignalProcessor, ValueKey};
use oscen_macros::Node;

/// A simple node that holds a value and passes it through.
/// This is useful for creating controllable parameters in the graph.
#[derive(Debug, Node)]
pub struct Value {
    #[input]
    input: f32,

    #[output]
    output: f32,
}

impl Value {
    pub fn new(initial_value: f32) -> Self {
        Self {
            input: initial_value,
            output: initial_value,
        }
    }
}

impl SignalProcessor for Value {
    #[inline(always)]
    fn process(&mut self) {
        // Simply pass through the input value
        self.output = self.input;
    }
}
