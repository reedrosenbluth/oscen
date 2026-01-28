use oscen::{InputEndpoint, Node, NodeKey, ProcessingNode, SignalProcessor, ValueKey};

/// AddValue - adds a value parameter to a stream signal.
/// Useful for adding envelope modulation to a base parameter value.
#[derive(Debug, Node)]
pub struct AddValue {
    #[input(stream)]
    pub input: f32,

    #[input(value)]
    pub value: f32,

    #[output(stream)]
    pub output: f32,
}

impl AddValue {
    pub fn new(value: f32) -> Self {
        Self {
            input: 0.0,
            value,
            output: 0.0,
        }
    }
}

impl Default for AddValue {
    fn default() -> Self {
        Self::new(0.0)
    }
}

impl SignalProcessor for AddValue {
    #[inline(always)]
    fn process(&mut self) {
        self.output = self.input + self.value;
    }
}
