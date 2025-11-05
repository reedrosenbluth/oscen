use oscen::{InputEndpoint, Node, NodeKey, ProcessingContext, ProcessingNode, SignalProcessor, ValueKey};
use std::f32::consts::PI;

/// Tremolo effect with stereo output
/// Creates a classic electric piano chorus/vibrato effect with complementary left/right modulation
#[derive(Debug, Node)]
#[allow(dead_code)]
pub struct Tremolo {
    #[input(stream)]
    input: f32,

    #[input(value)]
    rate: f32,

    #[input(value)]
    depth: f32,

    #[output(stream)]
    left_output: f32,

    #[output(stream)]
    right_output: f32,

    /// LFO phase (0.0 to 1.0)
    phase: f32,
    /// Sample rate
    sample_rate: f32,
}

impl Tremolo {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            input: 0.0,
            rate: 5.0,
            depth: 0.5,
            left_output: 0.0,
            right_output: 0.0,
            phase: 0.0,
            sample_rate,
        }
    }
}

impl SignalProcessor for Tremolo {
    fn init(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
    }

    fn process(&mut self, _sample_rate: f32, context: &mut ProcessingContext) -> f32 {
        // Get input audio
        let input = self.get_input(context);

        // Get tremolo parameters
        let rate = self.get_rate(context);
        let depth = self.get_depth(context);

        // Generate LFO (sine wave) that oscillates between 0 and 1
        let lfo = (self.phase * 2.0 * PI).sin();

        // Create complementary stereo panning effect like CMajor
        // The signal pans between left and right based on LFO
        // Scale depth to reasonable range (divide by larger number for subtler effect)
        let scaled_depth = depth / 3.0;
        let pan = 0.5 + lfo * scaled_depth; // Oscillates around center (0.5)

        // Apply constant-power panning
        self.left_output = input * pan;
        self.right_output = input * (1.0 - pan);

        // Advance phase
        let phase_increment = rate / self.sample_rate;
        self.phase = (self.phase + phase_increment).fract();

        // Return left channel as primary output
        self.left_output
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tremolo_creates() {
        let tremolo = Tremolo::new(44100.0);
        assert_eq!(tremolo.sample_rate, 44100.0);
        assert_eq!(tremolo.phase, 0.0);
    }
}
