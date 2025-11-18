use oscen::{InputEndpoint, Node, NodeKey, ProcessingNode, SignalProcessor, ValueKey};
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
    pub fn new() -> Self {
        Self {
            input: 0.0,
            rate: 5.0,
            depth: 0.5,
            left_output: 0.0,
            right_output: 0.0,
            phase: 0.0,
            sample_rate: 44100.0, // Will be set via init()
        }
    }
}

impl SignalProcessor for Tremolo {
    fn init(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
    }

    fn process(&mut self) {
        let input = self.input;
        let rate = self.rate;
        let depth = self.depth;

        // Generate LFO (sine wave) that oscillates between 0 and 1
        let lfo = (self.phase * 2.0 * PI).sin();

        // Create complementary stereo panning effect
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
