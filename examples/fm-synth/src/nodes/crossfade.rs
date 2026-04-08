use oscen::{Node, SignalProcessor, StreamInput, StreamOutput, ValueInput};

/// Crossfade - splits an input signal between two outputs based on a mix parameter.
///
/// When mix = 0.0: output_a = input, output_b = 0
/// When mix = 1.0: output_a = 0, output_b = input
/// Values in between blend linearly.
#[derive(Debug, Node)]
pub struct Crossfade {
    pub input: StreamInput,
    pub mix: ValueInput,

    pub output_a: StreamOutput,
    pub output_b: StreamOutput,
}

impl Crossfade {
    pub fn new() -> Self {
        Self {
            input: StreamInput::default(),
            mix: ValueInput::default(),
            output_a: StreamOutput::default(),
            output_b: StreamOutput::default(),
        }
    }
}

impl Default for Crossfade {
    fn default() -> Self {
        Self::new()
    }
}

impl SignalProcessor for Crossfade {
    #[inline(always)]
    fn process(&mut self) {
        let mix = self.mix.clamp(0.0, 1.0);
        let input = self.input;
        *self.output_a = input * (1.0 - mix);
        *self.output_b = input * mix;
    }
}
