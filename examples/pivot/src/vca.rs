use oscen::{Node, SignalProcessor, StreamInput, StreamOutput};

/// VCA (Voltage Controlled Amplifier) - multiplies two stream signals together.
/// Used to apply envelope modulation to audio signals.
#[derive(Debug, Node)]
pub struct Vca {
    pub input: StreamInput,

    pub control: StreamInput,

    pub output: StreamOutput,
}

impl Vca {
    pub fn new() -> Self {
        Self {
            input: StreamInput::default(),
            control: StreamInput(1.0),
            output: StreamOutput::default(),
        }
    }
}

impl Default for Vca {
    fn default() -> Self {
        Self::new()
    }
}

impl SignalProcessor for Vca {
    #[inline(always)]
    fn process(&mut self) {
        *self.output = self.input * self.control;
    }
}
