use oscen::{InputEndpoint, Node, NodeKey, ProcessingNode, SignalProcessor, ValueKey};

/// Crossfade - splits an input signal between two outputs based on a mix parameter.
///
/// When mix = 0.0: output_a = input, output_b = 0
/// When mix = 1.0: output_a = 0, output_b = input
/// Values in between blend linearly.
#[derive(Debug, Node)]
pub struct Crossfade {
    #[input(stream)]
    pub input: f32,
    #[input(value)]
    pub mix: f32,

    #[output(stream)]
    pub output_a: f32,
    #[output(stream)]
    pub output_b: f32,
}

impl Crossfade {
    pub fn new() -> Self {
        Self {
            input: 0.0,
            mix: 0.0,
            output_a: 0.0,
            output_b: 0.0,
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
        self.output_a = self.input * (1.0 - mix);
        self.output_b = self.input * mix;
    }
}
