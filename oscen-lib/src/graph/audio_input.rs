use super::traits::SignalProcessor;
use super::types::{StreamOutput, ValueInput};
use crate::Node;

/// AudioInput node
#[derive(Debug, Node)]
pub struct AudioInput {
    input_value: ValueInput,

    pub output: StreamOutput,
}

impl AudioInput {
    pub fn new() -> Self {
        Self {
            input_value: ValueInput::default(),
            output: StreamOutput::default(),
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
    fn process(&mut self) {
        // Simply pass through the input value
        *self.output = *self.input_value;
    }
}
