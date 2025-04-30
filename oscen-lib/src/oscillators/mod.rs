use crate::{
    EndpointDefinition, EndpointMetadata, InputEndpoint, Node, NodeKey, OutputEndpoint,
    ProcessingNode, SignalProcessor, ValueKey,
};
use std::f32::consts::PI;

#[derive(Debug, Node)]
pub struct Oscillator {
    #[input]
    phase: f32,
    #[input]
    frequency: f32,
    #[input]
    frequency_mod: f32,
    #[input]
    amplitude: f32,

    #[output]
    output: f32,

    waveform: fn(f32) -> f32,
}

impl Oscillator {
    pub fn new(frequency: f32, amplitude: f32, waveform: fn(f32) -> f32) -> Self {
        Self {
            phase: 0.0,
            frequency,
            frequency_mod: 0.0,
            amplitude,
            waveform,
            output: 0.0,
        }
    }

    pub fn sine(frequency: f32, amplitude: f32) -> Self {
        Self::new(frequency, amplitude, |p| (p * 2.0 * PI).sin())
    }

    pub fn square(frequency: f32, amplitude: f32) -> Self {
        Self::new(frequency, amplitude, |p| if p < 0.5 { 1.0 } else { -1.0 })
    }

    // Anti-aliased sawtooth using polynomial transition region
    pub fn saw(frequency: f32, amplitude: f32) -> Self {
        Self::new(frequency, amplitude, |p| {
            // Map phase from [0,1] to [-1,1]
            let x = p * 2.0 - 1.0;

            // Width of transition region (adjust for aliasing vs sharpness tradeoff)
            let transition_width = 0.1;

            // Linear ramp from -1 to 1
            let raw_saw = x;

            // Smooth transition near discontinuity using polynomial
            if x > (1.0 - transition_width) {
                let t = (x - (1.0 - transition_width)) / transition_width;
                -1.0 + (1.0 - t * t) * (raw_saw + 1.0)
            } else {
                raw_saw
            }
        })
    }
}

impl SignalProcessor for Oscillator {
    fn process(&mut self, sample_rate: f32, inputs: &[f32]) -> f32 {
        let phase_mod = self.get_phase(inputs);
        let freq_mod = self.get_frequency_mod(inputs);
        let freq_offset = self.get_frequency(inputs);
        let amp_mod = self.get_amplitude(inputs);

        // Use the initial frequency value when no input is connected
        let base_freq = if freq_offset == 0.0 {
            self.frequency
        } else {
            freq_offset
        };
        let frequency = base_freq * (1.0 + freq_mod);
        let amplitude = self.amplitude * (1.0 + amp_mod);

        let modulated_phase = (self.phase + phase_mod) % 1.0;
        self.output = (self.waveform)(modulated_phase) * amplitude;

        self.phase += frequency / sample_rate;
        self.phase %= 1.0; // Keep phase between 0 and 1

        self.output
    }
}