use oscen::{Node, SignalProcessor};

/// VCA (Voltage Controlled Amplifier) - multiplies two stream signals together.
/// Used to apply envelope modulation to audio signals.
#[derive(Debug, Node)]
pub struct Vca {
    #[input(stream)]
    pub input: f32,
    #[input(stream)]
    pub control: f32,
    #[output(stream)]
    pub output: f32,
}

impl Vca {
    pub fn new() -> Self {
        Self {
            input: Default::default(),
            control: 1.0,
            output: Default::default(),
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
        self.output = self.input * self.control;
    }
}
