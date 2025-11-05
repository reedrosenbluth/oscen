use oscen::{InputEndpoint, Node, NodeKey, ProcessingContext, ProcessingNode, SignalProcessor, ValueKey};
use std::f32::consts::PI;

const NUM_HARMONICS: usize = 32;

/// Harmonic oscillator bank that generates 32 harmonics using sine waves
#[derive(Debug, Node)]
pub struct HarmonicOscillatorBank {
    #[input(value)]
    frequency: f32,

    #[input(value)]
    amplitudes: [f32; NUM_HARMONICS],

    #[output(stream)]
    output: f32,

    /// Phase accumulators for each harmonic (0.0 to 1.0)
    phases: [f32; NUM_HARMONICS],
    /// Sample rate
    sample_rate: f32,
}

impl HarmonicOscillatorBank {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            frequency: 440.0,
            amplitudes: [0.0; NUM_HARMONICS],
            output: 0.0,
            phases: [0.0; NUM_HARMONICS],
            sample_rate,
        }
    }

    /// Reset all phases (called on note-on)
    #[allow(dead_code)]
    pub fn reset_phases(&mut self) {
        self.phases.fill(0.0);
    }
}

impl SignalProcessor for HarmonicOscillatorBank {
    fn init(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
    }

    fn process(&mut self, _sample_rate: f32, context: &mut ProcessingContext) -> f32 {
        // Get fundamental frequency and per-harmonic amplitudes from envelope
        let frequency = self.get_frequency(context);

        // Get the array of amplitudes from the value object
        // Use value_ref_amplitudes() to get the ValueRef, then downcast to array
        let envelope_amplitudes = self
            .value_ref_amplitudes(context)
            .and_then(|value_ref| value_ref.as_object())
            .and_then(|obj| obj.downcast_ref::<[f32; NUM_HARMONICS]>())
            .copied()
            .unwrap_or([0.0; NUM_HARMONICS]);

        let mut out = 0.0;

        // Process each harmonic
        for harmonic_idx in 0..NUM_HARMONICS {
            let harmonic_num = (harmonic_idx + 1) as f32;
            let harmonic_freq = frequency * harmonic_num;

            // Nyquist limiting - skip harmonics above Nyquist frequency
            if harmonic_freq >= self.sample_rate * 0.5 {
                continue;
            }

            // Get the per-harmonic envelope amplitude (this is the key change!)
            let amplitude = envelope_amplitudes[harmonic_idx];

            // Generate sine wave and apply per-harmonic envelope
            let phase = self.phases[harmonic_idx];
            let sample = (phase * 2.0 * PI).sin() * amplitude;
            out += sample;

            // Advance phase
            let phase_increment = harmonic_freq / self.sample_rate;
            self.phases[harmonic_idx] = (phase + phase_increment).fract();
        }

        // Normalize output by number of active harmonics to prevent clipping
        self.output = out / (NUM_HARMONICS as f32).sqrt();
        self.output
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_harmonic_bank_creates() {
        let bank = HarmonicOscillatorBank::new(44100.0);
        assert_eq!(bank.sample_rate, 44100.0);
        assert_eq!(bank.phases.len(), NUM_HARMONICS);
    }

    #[test]
    fn test_phases_reset() {
        let mut bank = HarmonicOscillatorBank::new(44100.0);
        bank.phases[0] = 0.5;
        bank.phases[10] = 0.75;
        bank.reset_phases();
        assert!(bank.phases.iter().all(|&p| p == 0.0));
    }
}
