use oscen::{Node, SampleRate, SignalProcessor};
use std::f32::consts::TAU;

/// FM Operator - a sine oscillator with phase modulation and self-feedback.
///
/// This is the core building block for FM synthesis. Each operator can:
/// - Generate a sine wave at a given base frequency multiplied by a ratio
/// - Accept external phase modulation from other operators
/// - Apply self-feedback to create richer timbres
#[derive(Debug, Node)]
pub struct FmOperator {
    phase: f32,
    prev_output: f32,
    sample_rate: SampleRate,

    #[input(value)]

    pub base_freq: f32,
    #[input(value)]
    pub ratio: f32,
    #[input(stream)]
    pub phase_mod: f32,
    #[input(value)]
    pub feedback: f32,
    #[output(stream)]

    pub output: f32,
}

impl FmOperator {
    pub fn new() -> Self {
        Self {
            phase: 0.0,
            prev_output: 0.0,
            sample_rate: SampleRate::default(),
            base_freq: 440.0,
            ratio: 1.0,
            phase_mod: Default::default(),
            feedback: Default::default(),
            output: Default::default(),
        }
    }
}

impl Default for FmOperator {
    fn default() -> Self {
        Self::new()
    }
}

impl SignalProcessor for FmOperator {
    #[inline(always)]
    fn process(&mut self) {
        // Calculate actual frequency from base and ratio
        let frequency = self.base_freq * self.ratio;

        // Calculate phase with external modulation and self-feedback
        let feedback_mod = self.prev_output * self.feedback;
        let total_phase_mod = self.phase_mod + feedback_mod;

        // Generate sine output
        let phase_rad = (self.phase + total_phase_mod) * TAU;
        let output = phase_rad.sin();
        self.output = output;
        self.prev_output = output;

        // Advance phase
        let phase_inc = frequency / *self.sample_rate;
        self.phase += phase_inc;
        self.phase = self.phase.fract();
    }
}
