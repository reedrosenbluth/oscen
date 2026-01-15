use oscen::{InputEndpoint, Node, NodeKey, ProcessingNode, SignalProcessor, ValueKey};
use std::f32::consts::PI;

/// A three-pole, 18dB/octave lowpass filter in the style of Rob Hordijk's TwinPeak filter.
#[allow(dead_code)]
#[derive(Debug, Node)]
pub struct LP18Filter {
    /// Input audio signal to be filtered
    #[input(stream)]
    pub input: f32,

    /// Cutoff frequency in Hz
    #[input(value)]
    pub cutoff: f32,

    /// frequency modulation input
    #[input(value)]
    pub fmod: f32,

    /// Resonance amount (0.0 to 1.0)
    #[input(value)]
    pub resonance: f32,

    /// Integrator memories for the three poles
    z: [f32; 3],

    /// Coefficients
    g: f32,
    h: f32,

    /// Cached values for change detection
    last_cutoff: f32,
    last_fmod: f32,
    last_resonance: f32,

    /// Sample rate
    sample_rate: f32,

    /// Filtered output signal
    #[output(stream)]
    pub output: f32,
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
            last_cutoff: cutoff,
            last_fmod: 0.0,
            last_resonance: resonance,
            sample_rate: 44100.0,
            output: 0.0,
        }
    }

    fn update_cutoff_coefficient(&mut self) {
        let modulated_cutoff = self.cutoff + self.fmod;
        let fc = (modulated_cutoff / self.sample_rate).clamp(0.001, 0.33);
        self.g = (PI * fc).tan();
    }

    fn update_resonance_coefficient(&mut self) {
        self.h = 2.0 * self.resonance;
    }
}

impl SignalProcessor for LP18Filter {
    fn init(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        self.update_cutoff_coefficient();
        self.update_resonance_coefficient();
    }

    fn process(&mut self) {
        // Check if cutoff or fmod changed
        if self.cutoff != self.last_cutoff || self.fmod != self.last_fmod {
            self.last_cutoff = self.cutoff;
            self.last_fmod = self.fmod;
            self.update_cutoff_coefficient();
        }

        // Check if resonance changed
        if self.resonance != self.last_resonance {
            self.last_resonance = self.resonance;
            self.resonance = self.resonance.clamp(0.0, 0.99);
            self.update_resonance_coefficient();
        }

        // 3-pole filter implementation (18dB/octave)
        let hp = (self.input - self.h * self.z[0] - self.z[1] - self.z[2]) / (1.0 + self.g);

        let bp1 = self.g * hp + self.z[0];
        self.z[0] = bp1.tanh();

        let bp2 = self.g * bp1 + self.z[1];
        self.z[1] = bp2;

        let lp = self.g * bp2 + self.z[2];
        self.z[2] = lp;

        self.output = lp;
    }
}
