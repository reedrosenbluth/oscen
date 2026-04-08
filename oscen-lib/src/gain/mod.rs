use crate::graph::{SignalProcessor, StreamInput, StreamOutput};
use crate::Node;

#[derive(Debug, Node)]
pub struct Gain {
    pub input: StreamInput,
    pub gain: StreamInput,
    pub output: StreamOutput,
}

impl Gain {
    pub fn new(initial_gain: f32) -> Self {
        Self {
            input: StreamInput::default(),
            gain: StreamInput(initial_gain),
            output: StreamOutput::default(),
        }
    }
}

impl Default for Gain {
    fn default() -> Self {
        Self::new(1.0)
    }
}

impl SignalProcessor for Gain {
    #[inline(always)]
    fn process(&mut self) {
        *self.output = *self.input * *self.gain;
    }
}
