use oscen::{Node, SignalProcessor, StreamInput, StreamOutput, ValueInput};

/// AddValue - adds a value parameter to a stream signal.
/// Useful for adding envelope modulation to a base parameter value.
#[derive(Debug, Node)]
pub struct AddValue {
    pub input: StreamInput,

    pub value: ValueInput,

    pub output: StreamOutput,
}

impl AddValue {
    pub fn new(value: f32) -> Self {
        Self {
            input: StreamInput::default(),
            value: ValueInput(value),
            output: StreamOutput::default(),
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
        *self.output = self.input + self.value;
    }
}
