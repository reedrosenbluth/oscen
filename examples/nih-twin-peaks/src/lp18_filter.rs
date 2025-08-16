use oscen::{
    EndpointDefinition, EndpointMetadata, InputEndpoint, Node, NodeKey,
    OutputEndpoint, ProcessingNode, SignalProcessor, ValueKey,
};
use std::f32::consts::PI;

/// A three-pole, 18dB/octave lowpass filter in the style of Rob Hordijk's TwinPeak filter.
#[derive(Debug, Node)]
pub struct LP18Filter {
    /// Input audio signal to be filtered
    #[input]
    input: f32,

    /// Cutoff frequency in Hz
    #[input]
    cutoff: f32,

    /// frequency modulation input
    #[input]
    fmod: f32,

    /// Resonance amount (0.0 to 1.0)
    #[input]
    resonance: f32,

    /// Integrator memories for the three poles
    z: [f32; 3],

    /// Coefficients
    g: f32,
    h: f32,

    /// Filtered output signal
    #[output]
    output: f32,
}

impl LP18Filter {
    pub fn new(cutoff: f32, resonance: f32) -> Self {
        Self {
            input: 0.0,
            cutoff,
            fmod: 0.0,
            resonance: resonance.clamp(0.0, 0.99),
            z: [0.0; 3],
            g: 0.0,
            h: 0.0,
            output: 0.0,
        }
    }

    fn update_cutoff_coefficient(&mut self, sample_rate: f32) {
        let fc = (self.cutoff / sample_rate).clamp(0.001, 0.499);
        self.g = (PI * fc).tan();
    }

    fn update_resonance_coefficient(&mut self) {
        self.h = 2.0 * self.resonance;
    }
}

impl SignalProcessor for LP18Filter {
    fn init(&mut self, sample_rate: f32) {
        self.update_cutoff_coefficient(sample_rate);
        self.update_resonance_coefficient();
    }

    fn process(&mut self, sample_rate: f32, inputs: &[f32]) -> f32 {
        let input = self.get_input(inputs);
        let cutoff = self.get_cutoff(inputs);
        let fmod = self.get_fmod(inputs);
        let resonance = self.get_resonance(inputs);

        let modulated_cutoff = cutoff + fmod;

        if cutoff != self.cutoff || fmod != self.fmod {
            self.cutoff = cutoff;
            self.fmod = fmod;

            let fc = (modulated_cutoff / sample_rate).clamp(0.001, 0.33);
            self.g = (PI * fc).tan();
        }

        if resonance != self.resonance {
            self.resonance = resonance.clamp(0.0, 0.99);
            self.update_resonance_coefficient();
        }

        // 3-pole filter implementation (18dB/octave)
        let hp = (input - self.h * self.z[0] - self.z[1] - self.z[2]) / (1.0 + self.g);

        let bp1 = self.g * hp + self.z[0];
        self.z[0] = bp1.tanh();

        let bp2 = self.g * bp1 + self.z[1];
        self.z[1] = bp2;

        let lp = self.g * bp2 + self.z[2];
        self.z[2] = lp;

        self.output = lp;
        lp
    }
}