use oscen::graph::types::EventPayload;
use oscen::{InputEndpoint, Node, NodeKey, ProcessingContext, ProcessingNode, SignalProcessor, ValueKey};

const NUM_HARMONICS: usize = 32;
const INTERPOLATION_STEPS: usize = 64;

/// Reference amplitude spectra for velocity 0 and velocity 127
/// These values are based on electric piano harmonic content
const VELOCITY_0_SPECTRUM: [f32; NUM_HARMONICS] = [
    1.00, 0.80, 0.65, 0.55, 0.48, 0.42, 0.37, 0.33, // Harmonics 1-8
    0.30, 0.27, 0.24, 0.22, 0.20, 0.18, 0.17, 0.15, // Harmonics 9-16
    0.14, 0.13, 0.12, 0.11, 0.10, 0.09, 0.08, 0.08, // Harmonics 17-24
    0.07, 0.06, 0.06, 0.05, 0.05, 0.04, 0.04, 0.03, // Harmonics 25-32
];

const VELOCITY_127_SPECTRUM: [f32; NUM_HARMONICS] = [
    1.00, 0.95, 0.88, 0.82, 0.76, 0.70, 0.65, 0.60, // Harmonics 1-8
    0.56, 0.52, 0.48, 0.45, 0.42, 0.39, 0.36, 0.34, // Harmonics 9-16
    0.31, 0.29, 0.27, 0.25, 0.23, 0.21, 0.20, 0.18, // Harmonics 17-24
    0.17, 0.15, 0.14, 0.13, 0.12, 0.11, 0.10, 0.09, // Harmonics 25-32
];

/// Per-harmonic envelope generator for electric piano
/// Generates 32 envelope values, one per harmonic
#[derive(Debug, Node)]
#[allow(dead_code)]
pub struct HarmonicEnvelope {
    #[input(event)]
    gate: (),

    #[input(value)]
    brightness: f32,

    #[input(value)]
    velocity_scaling: f32,

    #[input(value)]
    decay_rate: f32,

    #[input(value)]
    harmonic_decay: f32,

    #[input(value)]
    key_scaling: f32,

    #[input(value)]
    release_rate: f32,

    #[output(value)]
    amplitudes: [f32; NUM_HARMONICS], // Output the full array of per-harmonic amplitudes

    /// Current envelope amplitudes for each harmonic (internal working copy)
    amplitudes_internal: [f32; NUM_HARMONICS],
    /// Target amplitudes (for interpolation)
    target_amplitudes: [f32; NUM_HARMONICS],
    /// Interpolation counter for smooth transitions
    interpolation_step: usize,
    /// Is note currently active?
    is_active: bool,
    /// Current note velocity (0-1)
    velocity: f32,
    /// Current note pitch (MIDI note number)
    pitch: f32,
    /// Sample rate
    sample_rate: f32,
}

impl HarmonicEnvelope {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            gate: (),
            brightness: 0.5,
            velocity_scaling: 0.5,
            decay_rate: 0.5,
            harmonic_decay: 0.5,
            key_scaling: 0.5,
            release_rate: 0.5,
            amplitudes: [0.0; NUM_HARMONICS],
            amplitudes_internal: [0.0; NUM_HARMONICS],
            target_amplitudes: [0.0; NUM_HARMONICS],
            interpolation_step: INTERPOLATION_STEPS,
            is_active: false,
            velocity: 0.0,
            pitch: 60.0,
            sample_rate,
        }
    }

    fn trigger_note(&mut self, velocity: f32, brightness: f32, velocity_scaling: f32) {
        self.is_active = true;
        self.velocity = velocity;

        // Blend between velocity_0 and velocity_127 spectra based on velocity
        let vel_blend = velocity * velocity_scaling;

        for i in 0..NUM_HARMONICS {
            let base_amp =
                VELOCITY_0_SPECTRUM[i] * (1.0 - vel_blend) + VELOCITY_127_SPECTRUM[i] * vel_blend;

            // Apply brightness scaling to higher harmonics
            let harmonic_num = (i + 1) as f32;
            let brightness_scale =
                1.0 - (1.0 - brightness) * (harmonic_num / NUM_HARMONICS as f32);

            self.target_amplitudes[i] = base_amp * brightness_scale;
        }

        // Start interpolation
        self.interpolation_step = 0;
    }

    fn release_note(&mut self) {
        self.is_active = false;
        // Set targets to zero for release
        self.target_amplitudes.fill(0.0);
        self.interpolation_step = 0;
    }
}

impl SignalProcessor for HarmonicEnvelope {
    fn init(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
    }

    fn process(&mut self, _sample_rate: f32, context: &mut ProcessingContext) -> f32 {
        // Handle gate events
        for event in self.events_gate(context).iter() {
            match &event.payload {
                EventPayload::Scalar(velocity) if *velocity > 0.0 => {
                    // Note on
                    let brightness = self.get_brightness(context);
                    let velocity_scaling = self.get_velocity_scaling(context);
                    // Extract MIDI note number from velocity if it's encoded
                    // For now, we'll use a fixed middle C
                    self.pitch = 60.0;
                    self.trigger_note(*velocity, brightness, velocity_scaling);
                }
                _ => {
                    // Note off
                    self.release_note();
                }
            }
        }

        // Get envelope parameters
        let decay_rate = self.get_decay_rate(context);
        let harmonic_decay = self.get_harmonic_decay(context);
        let key_scaling = self.get_key_scaling(context);
        let release_rate = self.get_release_rate(context);

        // Calculate decay rate with key scaling
        // Lower notes decay slower (key scaling makes higher notes decay faster)
        let pitch_factor = 1.0 + key_scaling * ((self.pitch - 60.0) / 60.0);

        // Process each harmonic envelope
        for i in 0..NUM_HARMONICS {
            // Interpolate to target amplitude
            if self.interpolation_step < INTERPOLATION_STEPS {
                let t = self.interpolation_step as f32 / INTERPOLATION_STEPS as f32;
                self.amplitudes_internal[i] = self.amplitudes_internal[i] * (1.0 - t)
                    + self.target_amplitudes[i] * t;
            }

            // Apply decay/release
            if self.is_active {
                // Decay phase - higher harmonics decay faster
                let harmonic_num = (i + 1) as f32;
                let harmonic_factor =
                    1.0 + harmonic_decay * (harmonic_num / NUM_HARMONICS as f32);
                let decay_coefficient =
                    (-decay_rate * 8.0 * harmonic_factor * pitch_factor / self.sample_rate).exp();
                self.amplitudes_internal[i] *= decay_coefficient;
            } else {
                // Release phase - all harmonics use same release rate
                let release_coefficient = (-release_rate * 4.0 / self.sample_rate).exp();
                self.amplitudes_internal[i] *= release_coefficient;
            }
        }

        // Increment interpolation step
        if self.interpolation_step < INTERPOLATION_STEPS {
            self.interpolation_step += 1;
        }

        // Output the full array of per-harmonic amplitudes
        self.amplitudes = self.amplitudes_internal;

        // Return average amplitude as the scalar return value (for monitoring)
        self.amplitudes_internal.iter().sum::<f32>() / NUM_HARMONICS as f32
    }

    fn is_active(&self) -> bool {
        // Envelope is active if note is on or any amplitude is non-zero
        self.is_active || self.amplitudes_internal.iter().any(|&a| a > 0.001)
    }
}
