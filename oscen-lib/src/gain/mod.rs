use crate::graph::{InputEndpoint, NodeKey, ProcessingNode, SignalProcessor, ValueKey};
use crate::Node;

#[derive(Debug, Node)]
pub struct Gain {
    #[input(stream)]
    pub input: f32,

    #[input(value)]
    pub gain: f32,

    #[output(stream)]
    pub output: f32,
}

impl Gain {
    pub fn new(initial_gain: f32) -> Self {
        Self {
            input: 0.0,
            gain: initial_gain,
            output: 0.0,
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
        // Inputs already populated in self.input and self.gain
        self.output = self.input * self.gain;
    }
}
