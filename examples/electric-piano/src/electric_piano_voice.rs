use oscen::graph::types::EventPayload;
use oscen::{
    InputEndpoint, Node, NodeKey, ProcessingContext, ProcessingNode, SignalProcessor, ValueKey,
};
use std::f32::consts::PI;

const NUM_HARMONICS: usize = 32;
const INTERPOLATION_STEPS: usize = 64;

/// Reference amplitude spectra for velocity 0 and velocity 127
/// These are sampled from actual electric piano sounds
const VELOCITY_0_SPECTRUM: [f32; NUM_HARMONICS] = [
    0.02, 0.05, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
    0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
];

const VELOCITY_127_SPECTRUM: [f32; NUM_HARMONICS] = [
    0.150869,
    0.385766,
    0.215543,
    0.117811,
    0.100411,
    0.0128637,
    0.0288844,
    0.00243388,
    0.00963092,
    0.0035634,
    0.00256945,
    0.00184799,
    0.000399878,
    0.000660576,
    3.00995e-05,
    0.00021866,
    9.33705e-05,
    0.000177973,
    0.0002545,
    0.000323602,
    0.000779045,
    0.000116569,
    0.000772873,
    0.000364486,
    0.000248027,
    0.00018236,
    3.27292e-05,
    6.64988e-05,
    0.0,
    0.0,
    0.0,
    0.0,
];

/// Combined electric piano voice with per-harmonic oscillators and envelopes
/// This matches the CMajor implementation where each harmonic has its own envelope
/// that is applied to its oscillator output before summing
#[derive(Debug, Node)]
pub struct ElectricPianoVoiceNode {
    #[input(value)]
    frequency: f32,

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

    #[output(stream)]
    output: f32,

    /// Phase accumulators for each harmonic (0.0 to 1.0)
    phases: [f32; NUM_HARMONICS],
    /// Current envelope amplitudes for each harmonic
    amplitudes: [f32; NUM_HARMONICS],
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

impl ElectricPianoVoiceNode {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            frequency: 440.0,
            gate: (),
            brightness: 0.5,
            velocity_scaling: 0.5,
            decay_rate: 0.5,
            harmonic_decay: 0.5,
            key_scaling: 0.5,
            release_rate: 0.5,
            output: 0.0,
            phases: [0.0; NUM_HARMONICS],
            amplitudes: [0.0; NUM_HARMONICS],
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

        // Reset phases on note-on
        self.phases.fill(0.0);

        // Blend between velocity_0 and velocity_127 spectra based on velocity
        let vel_blend = velocity * velocity_scaling;

        for i in 0..NUM_HARMONICS {
            let base_amp =
                VELOCITY_0_SPECTRUM[i] * (1.0 - vel_blend) + VELOCITY_127_SPECTRUM[i] * vel_blend;

            // Apply brightness scaling to higher harmonics
            let harmonic_num = (i + 1) as f32;
            let brightness_scale = 1.0 - (1.0 - brightness) * (harmonic_num / NUM_HARMONICS as f32);

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

impl SignalProcessor for ElectricPianoVoiceNode {
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
                    // TODO: Extract MIDI note number from event for pitch-based key scaling
                    self.pitch = 60.0;
                    self.trigger_note(*velocity, brightness, velocity_scaling);
                }
                _ => {
                    // Note off
                    self.release_note();
                }
            }
        }

        // Get parameters
        let frequency = self.get_frequency(context);
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
                self.amplitudes[i] = self.amplitudes[i] * (1.0 - t) + self.target_amplitudes[i] * t;
            }

            // Apply decay/release
            if self.is_active {
                // Decay phase - higher harmonics decay faster
                let harmonic_num = (i + 1) as f32;
                let harmonic_factor = 1.0 + harmonic_decay * (harmonic_num / NUM_HARMONICS as f32);
                let decay_coefficient =
                    (-decay_rate * 8.0 * harmonic_factor * pitch_factor / self.sample_rate).exp();
                self.amplitudes[i] *= decay_coefficient;
            } else {
                // Release phase - all harmonics use same release rate
                let release_coefficient = (-release_rate * 4.0 / self.sample_rate).exp();
                self.amplitudes[i] *= release_coefficient;
            }
        }

        // Increment interpolation step
        if self.interpolation_step < INTERPOLATION_STEPS {
            self.interpolation_step += 1;
        }

        // Generate output by summing harmonics weighted by their envelopes
        let mut out = 0.0;
        for harmonic_idx in 0..NUM_HARMONICS {
            let harmonic_num = (harmonic_idx + 1) as f32;
            let harmonic_freq = frequency * harmonic_num;

            // Nyquist limiting - skip harmonics above Nyquist frequency
            if harmonic_freq >= self.sample_rate * 0.5 {
                continue;
            }

            // Generate sine wave and apply per-harmonic envelope (this is the key!)
            let phase = self.phases[harmonic_idx];
            let sample = (phase * 2.0 * PI).sin() * self.amplitudes[harmonic_idx];
            out += sample;

            // Advance phase
            let phase_increment = harmonic_freq / self.sample_rate;
            self.phases[harmonic_idx] = (phase + phase_increment).fract();
        }

        self.output = out * 3.0;
        self.output
    }

    fn is_active(&self) -> bool {
        // Voice is active if note is on or any amplitude is non-zero
        self.is_active || self.amplitudes.iter().any(|&a| a > 0.001)
    }
}
