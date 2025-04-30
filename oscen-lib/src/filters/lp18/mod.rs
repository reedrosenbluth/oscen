use crate::{
    EndpointDefinition, EndpointMetadata, InputEndpoint, Node, NodeKey, OutputEndpoint,
    ProcessingNode, SignalProcessor, ValueKey,
};
use std::f32::consts::PI;

/// A three-pole, 18dB/octave lowpass filter in the style of Rob Hordijk's TwinPeak filter.
/// This filter uses three identical TPT (Trapezoidal) integrators in series.
#[derive(Debug, Node)]
pub struct LP18Filter {
    /// Input audio signal to be filtered
    #[input]
    input: f32,

    /// Cutoff frequency in Hz
    #[input]
    cutoff: f32,

    /// Resonance amount (0.0 to 1.0)
    #[input]
    resonance: f32,

    /// Integrator memories for the three poles
    z: [f32; 3],

    /// Raw coefficient (g = tan(π · Fc / Fs))
    g: f32,

    /// Pre-multiplier for speed (h = g / (1 + g))
    h: f32,

    /// Frame counting for coefficient updates
    frame_counter: usize,
    frames_per_update: usize,

    /// Filtered output signal
    #[output]
    output: f32,
}

impl LP18Filter {
    /// Creates a new LP18Filter instance with the specified cutoff frequency and resonance
    pub fn new(cutoff: f32, resonance: f32) -> Self {
        Self {
            input: 0.0,
            cutoff,
            resonance: resonance.clamp(0.0, 0.99), // Prevent excessive resonance
            z: [0.0; 3],
            g: 0.0,
            h: 0.0,
            frame_counter: 0,
            frames_per_update: 32, // Update coefficients every 32 samples (control rate)
            output: 0.0,
        }
    }

    /// Creates a new LP18Filter with specified cutoff and no resonance
    pub fn new_simple(cutoff: f32) -> Self {
        Self::new(cutoff, 0.0)
    }

    /// Updates the filter coefficients based on the current cutoff frequency
    fn update_coefficients(&mut self, sample_rate: f32) {
        // Clamp cutoff to valid range (0, Fs/2 - ε)
        let freq = self.cutoff.clamp(1.0, sample_rate * 0.45);

        // adjust by 2^(1/3)
        let adjusted_freq = freq * 1.25992;

        // Calculate coefficients: g = tan(π · Fc / Fs)
        self.g = (PI * adjusted_freq / sample_rate).tan();

        // Pre-calculate h = g / (1 + g) for optimization
        self.h = self.g / (1.0 + self.g);

        // Limit coefficient to prevent instability at high frequencies
        if self.h > 0.99 {
            self.h = 0.99;
            self.g = self.h / (1.0 - self.h);
        }

        // If cutoff is very low, zero the z states to avoid denormals
        if freq < 1.0 {
            self.z = [0.0; 3];
        }
    }
}

impl SignalProcessor for LP18Filter {
    fn init(&mut self, sample_rate: f32) {
        // Initialize filter coefficients
        self.update_coefficients(sample_rate);
    }

    fn process(&mut self, sample_rate: f32, inputs: &[f32]) -> f32 {
        // Get input values
        let audio_in = self.get_input(inputs);
        let resonance = self.get_resonance(inputs);

        // Update coefficients at control rate
        if self.frame_counter == 0 {
            let cutoff = self.get_cutoff(inputs);

            // Only recalculate if cutoff changed
            if cutoff != self.cutoff {
                self.cutoff = cutoff;
                self.update_coefficients(sample_rate);
            }
        }

        self.frame_counter = (self.frame_counter + 1) % self.frames_per_update;

        // Calculate resonance feedback from previous output
        let feedback = self.output * resonance;

        // Sum audio_in with resonance feedback
        let x = audio_in + feedback;

        // Implement the three-pole filter algorithm (TPT integrators in series)
        // v1 = (x  - z1) * h ;  y1 = v1 + z1 ;  z1 = y1 + v1
        let v1 = (x - self.z[0]) * self.h;
        let y1 = v1 + self.z[0];
        self.z[0] = y1 + v1;

        // v2 = (y1 - z2) * h ;  y2 = v2 + z2 ;  z2 = y2 + v2
        let v2 = (y1 - self.z[1]) * self.h;
        let y2 = v2 + self.z[1];
        self.z[1] = y2 + v2;

        // v3 = (y2 - z3) * h ;  y  = v3 + z3 ;  z3 = y  + v3
        let v3 = (y2 - self.z[2]) * self.h;
        let y = v3 + self.z[2];
        self.z[2] = y + v3;

        // Output is the result of the third pole
        self.output = y;
        y
    }
}

#[cfg(test)]
mod tests;
