use oscen::{InputEndpoint, Node, NodeKey, ProcessingNode, SignalProcessor, ValueKey};
use std::f32::consts::TAU;

/// FM Operator - a sine oscillator with phase modulation and self-feedback.
///
/// This is the core building block for FM synthesis. Each operator can:
/// - Generate a sine wave at a given base frequency multiplied by a ratio
/// - Accept external phase modulation from other operators
/// - Apply self-feedback to create richer timbres
/// - Apply an amplitude envelope and output level
#[derive(Debug, Node)]
pub struct FmOperator {
    phase: f32,
    prev_output: f32,
    sample_rate: f32,

    #[input(value)]
    pub base_freq: f32,
    #[input(value)]
    pub ratio: f32,
    #[input(stream)]
    pub phase_mod: f32,
    #[input(value)]
    pub feedback: f32,
    #[input(stream)]
    pub envelope: f32,
    #[input(value)]
    pub level: f32,

    #[output(stream)]
    pub output: f32,
}

impl FmOperator {
    pub fn new() -> Self {
        Self {
            phase: 0.0,
            prev_output: 0.0,
            sample_rate: 44100.0,
            base_freq: 440.0,
            ratio: 1.0,
            phase_mod: 0.0,
            feedback: 0.0,
            envelope: 1.0,
            level: 1.0,
            output: 0.0,
        }
    }
}

impl Default for FmOperator {
    fn default() -> Self {
        Self::new()
    }
}

impl SignalProcessor for FmOperator {
    fn init(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
    }

    #[inline(always)]
    fn process(&mut self) {
        // Calculate actual frequency from base and ratio
        let frequency = self.base_freq * self.ratio;

        // Calculate phase with external modulation and self-feedback
        let feedback_mod = self.prev_output * self.feedback;
        let total_phase_mod = self.phase_mod + feedback_mod;

        // Generate sine output with envelope and level applied
        let phase_rad = (self.phase + total_phase_mod) * TAU;
        self.output = phase_rad.sin() * self.envelope * self.level;
        self.prev_output = self.output;

        // Advance phase
        let phase_inc = frequency / self.sample_rate;
        self.phase += phase_inc;
        self.phase = self.phase.fract();
    }

    fn allows_feedback(&self) -> bool {
        true
    }
}
