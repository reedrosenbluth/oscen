use crate::graph::{
    InputEndpoint, NodeKey, ProcessingContext, ProcessingNode, SignalProcessor, ValueKey,
};
use crate::Node;

#[derive(Debug, Node)]
pub struct Gain {
    #[input(stream)]
    input: f32,

    #[input(value)]
    gain: f32,

    #[output(stream)]
    output: f32,
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
    fn process<'a>(&mut self, _sample_rate: f32, context: &mut ProcessingContext<'a>) -> f32 {
        let input = self.get_input(context);
        let gain = self.get_gain(context);
        self.output = input * gain;
        self.output
    }
}
