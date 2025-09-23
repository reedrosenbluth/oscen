use super::traits::{ProcessingContext, SignalProcessor};
use super::{EndpointType, InputEndpoint, NodeKey, OutputEndpoint, ProcessingNode, ValueKey};
use crate::Node;

#[derive(Debug, Node)]
pub struct AudioInput {
    #[input]
    input_value: f32,

    #[output(stream)]
    output: f32,
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
    fn process<'a>(&mut self, _sample_rate: f32, context: &mut ProcessingContext<'a>) -> f32 {
        let input_val = self.get_input_value(context);
        self.output = input_val;
        self.output
    }
}
