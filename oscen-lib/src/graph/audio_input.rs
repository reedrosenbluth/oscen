use super::traits::SignalProcessor;
use super::{InputEndpoint, NodeKey, ProcessingNode, ValueKey};
use crate::Node;

/// AudioInput node
#[derive(Debug, Node)]
pub struct AudioInput {
    #[input]
    input_value: f32,

    #[output(stream)]
    pub output: f32,
}

impl AudioInput {
    pub fn new() -> Self {
        Self {
            input_value: 0.0,
            output: 0.0,
        }
    }
}

impl Default for AudioInput {
    fn default() -> Self {
        Self::new()
    }
}

impl SignalProcessor for AudioInput {
    #[inline(always)]
    fn process(&mut self, _sample_rate: f32) {
        // Simply pass through the input value
        self.output = self.input_value;
    }
}
