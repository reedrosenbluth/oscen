use oscen::{InputEndpoint, Node, NodeKey, ProcessingNode, SignalProcessor, ValueKey};

/// Mixer - adds two stream inputs together.
#[derive(Debug, Node)]
pub struct Mixer {
    #[input(stream)]
    pub input_a: f32,
    #[input(stream)]
    pub input_b: f32,

    #[output(stream)]
    pub output: f32,
}

impl Mixer {
    pub fn new() -> Self {
        Self {
            input_a: 0.0,
            input_b: 0.0,
            output: 0.0,
        }
    }
}

impl Default for Mixer {
    fn default() -> Self {
        Self::new()
    }
}

impl SignalProcessor for Mixer {
    #[inline(always)]
    fn process(&mut self) {
        self.output = self.input_a + self.input_b;
    }
}
