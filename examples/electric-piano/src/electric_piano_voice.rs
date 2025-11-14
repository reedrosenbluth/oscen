use oscen::graph::types::{EventInstance, EventPayload};
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

/// Complex number for efficient oscillator implementation
/// Using complex rotation is far more efficient than calling sin() 32 times per sample
#[derive(Debug, Clone, Copy)]
struct Complex {
    real: f32,
    imag: f32,
}

impl Complex {
    fn new(real: f32, imag: f32) -> Self {
        Self { real, imag }
    }

    fn one() -> Self {
        Self::new(1.0, 0.0)
    }

    /// Multiply this complex number by another (rotation in the complex plane)
    #[inline(always)]
    fn mul(&mut self, other: &Complex) {
        let new_real = self.real * other.real - self.imag * other.imag;
        let new_imag = self.real * other.imag + self.imag * other.real;
        self.real = new_real;
        self.imag = new_imag;
    }
}

/// OscillatorBank generates NUM_HARMONICS sine wave harmonics using complex number rotation.
#[derive(Debug, Node)]
pub struct OscillatorBank {
    #[input(value)]
    frequency: f32,

    #[input(value)]
    amplitudes: [f32; NUM_HARMONICS],

    #[output(stream)]
    output: f32,

    /// Complex oscillators (one per harmonic) - rotated each sample
    oscillators: [Complex; NUM_HARMONICS],
    /// Rotation multipliers (precomputed based on frequency)
    multipliers: [Complex; NUM_HARMONICS],
    /// Last frequency we computed multipliers for
    last_frequency: f32,

    sample_rate: f32,
}

impl OscillatorBank {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            frequency: 440.0,
            amplitudes: [0.0; NUM_HARMONICS],
            output: 0.0,
            oscillators: [Complex::one(); NUM_HARMONICS],
            multipliers: [Complex::one(); NUM_HARMONICS],
            last_frequency: 0.0,
            sample_rate,
        }
    }

    /// Set the rotation multipliers based on fundamental frequency
    /// Only recompute when frequency changes
    fn update_multipliers(&mut self, note_frequency: f32) {
        if (self.last_frequency - note_frequency).abs() < 0.01 {
            return; // No significant change
        }

        self.last_frequency = note_frequency;
        let nyquist = self.sample_rate * 0.5;

        for i in 0..NUM_HARMONICS {
            let harmonic_num = (i + 1) as f32;
            let harmonic_freq = note_frequency * harmonic_num;

            if harmonic_freq < nyquist {
                let angle = 2.0 * PI * harmonic_freq / self.sample_rate;
                self.multipliers[i] = Complex::new(angle.cos(), angle.sin());
            } else {
                // Above Nyquist - set to identity (no rotation)
                self.multipliers[i] = Complex::one();
            }
        }

        // Reset oscillators on frequency change
        self.oscillators = [Complex::one(); NUM_HARMONICS];
    }
}

impl SignalProcessor for OscillatorBank {
    fn init(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
    }

    fn process(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;

        // Update multipliers if frequency changed
        if self.frequency > 0.0 {
            self.update_multipliers(self.frequency);
        }

        // Rotate all oscillators and sum their imaginary parts weighted by amplitudes
        // This is the key optimization: one complex multiply per harmonic instead of sin()
        let mut sum = 0.0;
        for i in 0..NUM_HARMONICS {
            self.oscillators[i].mul(&self.multipliers[i]);
            sum += self.oscillators[i].imag * self.amplitudes[i];
        }

        self.output = sum * 3.0; // Output scaling to match original
    }
}

/// AmplitudeSource generates per-harmonic envelope values.
#[derive(Debug, Node)]
pub struct AmplitudeSource {
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

    #[output(value)]
    amplitudes: [f32; NUM_HARMONICS],

    /// Current amplitude values for each harmonic
    current_value: [f32; NUM_HARMONICS],
    /// Decay multipliers per harmonic (applied when note is held)
    decay: [f32; NUM_HARMONICS],
    /// Release multipliers per harmonic (applied after note off)
    release: [f32; NUM_HARMONICS],
    /// Whether note has been released
    released: bool,
    /// Current note pitch (MIDI note number)
    note_pitch: f32,
    /// Current velocity (0-1)
    velocity: f32,
    /// Interpolation step counter
    interpolation_step: usize,
}

impl AmplitudeSource {
    pub fn new() -> Self {
        Self {
            frequency: 440.0,
            gate: (),
            brightness: 0.5,
            velocity_scaling: 0.5,
            decay_rate: 0.5,
            harmonic_decay: 0.5,
            key_scaling: 0.5,
            release_rate: 0.5,
            amplitudes: [0.0; NUM_HARMONICS],
            current_value: [0.0; NUM_HARMONICS],
            decay: [0.0; NUM_HARMONICS],
            release: [0.0; NUM_HARMONICS],
            released: false,
            note_pitch: 60.0,
            velocity: 0.0,
            interpolation_step: INTERPOLATION_STEPS,
        }
    }

    fn get_decay(&self, note: f32) -> [f32; NUM_HARMONICS] {
        let base_decay_rate = self.decay_rate / 40000.0;
        let harmonic_scaling = 1.0 - (self.harmonic_decay / 200000.0);

        let scaling_multiplier = (48.0 - note) / 12.0;
        let key_scaling_factor = scaling_multiplier * (self.key_scaling * 0.02);

        let adjusted_decay = if key_scaling_factor > 0.0 {
            1.0 - (base_decay_rate / (1.0 + key_scaling_factor))
        } else {
            1.0 - (base_decay_rate * (1.0 - key_scaling_factor))
        };

        let mut decay = [0.0; NUM_HARMONICS];
        let mut scaling = 1.0;

        for i in 0..NUM_HARMONICS {
            decay[i] = adjusted_decay * scaling;
            scaling *= harmonic_scaling;
        }

        decay
    }

    fn get_release(&self, _note: f32) -> [f32; NUM_HARMONICS] {
        let release_value = 0.999 - (self.release_rate / 1000.0);
        [release_value; NUM_HARMONICS]
    }

    fn get_initial_amplitudes(&self, _note: f32, velocity: f32) -> [f32; NUM_HARMONICS] {
        // Blend between velocity 0 and velocity 127 spectra
        let mut amplitudes = [0.0; NUM_HARMONICS];
        for i in 0..NUM_HARMONICS {
            amplitudes[i] =
                (VELOCITY_127_SPECTRUM[i] * velocity) + (VELOCITY_0_SPECTRUM[i] * (1.0 - velocity));
        }

        let mut brightness_scaling = -0.2 + (0.8 * (self.brightness * 0.01));
        brightness_scaling += velocity * self.velocity_scaling * 0.01 * 0.5;

        for i in 0..NUM_HARMONICS {
            amplitudes[i] *= 1.0 + brightness_scaling * (i as f32);
        }

        amplitudes
    }

    fn trigger_note(&mut self, velocity: f32) {
        self.velocity = velocity;
        self.decay = self.get_decay(self.note_pitch);
        self.release = self.get_release(self.note_pitch);
        self.current_value = self.get_initial_amplitudes(self.note_pitch, velocity);
        self.released = false;
        self.interpolation_step = 0;
    }

    fn release_note(&mut self) {
        self.released = true;
        self.interpolation_step = 0;
    }
}

impl AmplitudeSource {
    fn on_gate(&mut self, event: &EventInstance, _context: &mut ProcessingContext) {
        match &event.payload {
            EventPayload::Scalar(velocity) if *velocity > 0.0 => {
                self.trigger_note(*velocity);
            }
            _ => {
                self.release_note();
            }
        }
    }
}

impl SignalProcessor for AmplitudeSource {
    fn process(&mut self, _sample_rate: f32) {
        // Use decay or release multipliers based on note state
        let multiplier = if self.released {
            self.release
        } else {
            self.decay
        };

        // Calculate target values
        let mut target = [0.0; NUM_HARMONICS];
        for i in 0..NUM_HARMONICS {
            target[i] = self.current_value[i] * multiplier[i];
        }

        if self.interpolation_step < INTERPOLATION_STEPS {
            let t = self.interpolation_step as f32 / INTERPOLATION_STEPS as f32;
            for i in 0..NUM_HARMONICS {
                self.current_value[i] = self.current_value[i] * (1.0 - t) + target[i] * t;
            }
            self.interpolation_step += 1;
        } else {
            // Reset interpolation for next cycle
            self.current_value = target;
            self.interpolation_step = 0;
        }

        self.amplitudes = self.current_value;
    }

    fn is_active(&self) -> bool {
        !self.released || self.current_value.iter().any(|&a| a > 0.001)
    }
}

// Voice graph using graph! macro
// This is now a proper runtime graph that connects OscillatorBank and AmplitudeSource
use oscen::graph;

graph! {
    name: ElectricPianoVoiceNode;

    input value frequency = 440.0;
    input event gate;
    input value brightness = 0.5;
    input value velocity_scaling = 0.5;
    input value decay_rate = 0.5;
    input value harmonic_decay = 0.5;
    input value key_scaling = 0.5;
    input value release_rate = 0.5;

    output stream output;

    node {
        amplitude_source = crate::electric_piano_voice::AmplitudeSource::new();
        oscillator_bank = crate::electric_piano_voice::OscillatorBank::new(sample_rate);
    }

    connection {
        // Forward parameters to amplitude source
        frequency -> amplitude_source.frequency;
        gate -> amplitude_source.gate;
        brightness -> amplitude_source.brightness;
        velocity_scaling -> amplitude_source.velocity_scaling;
        decay_rate -> amplitude_source.decay_rate;
        harmonic_decay -> amplitude_source.harmonic_decay;
        key_scaling -> amplitude_source.key_scaling;
        release_rate -> amplitude_source.release_rate;

        // Forward frequency to oscillator bank
        frequency -> oscillator_bank.frequency;

        // Connect amplitude source output to oscillator bank input (key connection!)
        amplitude_source.amplitudes -> oscillator_bank.amplitudes;

        // Route oscillator output to voice output
        oscillator_bank.output -> output;
    }
}
