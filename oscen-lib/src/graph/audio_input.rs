use super::traits::{ProcessingContext, SignalProcessor};
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
    fn process<'a>(
        &mut self,
        _sample_rate: f32,
        context: &mut ProcessingContext<'a>,
    ) {
        // Get value input from graph
        let input_val = self.get_input_value(context);

        // Write to output field
        self.output = input_val;
    }

    fn get_stream_output(&self, index: usize) -> Option<f32> {
        match index {
            0 => Some(self.output),
            _ => None,
        }
    }
}
