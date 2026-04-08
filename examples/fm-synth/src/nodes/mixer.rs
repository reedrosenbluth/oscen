use oscen::{Node, SignalProcessor, StreamInput, StreamOutput};

/// Mixer - adds two stream inputs together.
#[derive(Debug, Node)]
pub struct Mixer {
    pub input_a: StreamInput,
    pub input_b: StreamInput,

    pub output: StreamOutput,
}

impl Mixer {
    pub fn new() -> Self {
        Self {
            input_a: StreamInput::default(),
            input_b: StreamInput::default(),
            output: StreamOutput::default(),
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
        *self.output = self.input_a + self.input_b;
    }
}
