use crate::{
    EndpointType, InputEndpoint, Node, NodeKey, OutputEndpoint, ProcessingNode, SignalProcessor,
    ValueKey,
};
use std::f32::consts::{PI, TAU};

#[allow(dead_code)]
#[derive(Debug, Node)]
pub struct Oscillator {
    #[input(value)]
    phase: f32,
    #[input(value)]
    frequency: f32,
    #[input(stream)]
    frequency_mod: f32,
    #[input(value)]
    amplitude: f32,

    #[output(stream)]
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
            // Width of transition region (adjust for aliasing vs sharpness tradeoff)
            let transition_width = 0.1;

            // Linear ramp from -1 to 1 (over one full cycle)
            let raw_saw = 2.0 * p - 1.0;

            // Smooth transition near discontinuity using polynomial
            if p > (1.0 - transition_width / 2.0) {
                let t = (p - (1.0 - transition_width / 2.0)) / (transition_width / 2.0);
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

#[derive(Debug, Clone, Copy)]
pub enum PolyBlepWaveform {
    Sine,
    Saw,
    Square,
    Triangle,
}

#[allow(dead_code)]
#[derive(Debug, Node)]
pub struct PolyBlepOscillator {
    #[input(value)]
    phase: f32,
    #[input(value)]
    frequency: f32,
    #[input(stream)]
    frequency_mod: f32,
    #[input(value)]
    amplitude: f32,
    #[input(value)]
    pulse_width: f32,

    #[output(stream)]
    output: f32,

    waveform: PolyBlepWaveform,
}

impl PolyBlepOscillator {
    pub fn new(frequency: f32, amplitude: f32, waveform: PolyBlepWaveform) -> Self {
        Self {
            phase: 0.0,
            frequency,
            frequency_mod: 0.0,
            amplitude,
            pulse_width: 0.5,
            output: 0.0,
            waveform,
        }
    }

    pub fn sine(frequency: f32, amplitude: f32) -> Self {
        Self::new(frequency, amplitude, PolyBlepWaveform::Sine)
    }

    pub fn saw(frequency: f32, amplitude: f32) -> Self {
        Self::new(frequency, amplitude, PolyBlepWaveform::Saw)
    }

    pub fn square(frequency: f32, amplitude: f32) -> Self {
        Self::new(frequency, amplitude, PolyBlepWaveform::Square)
    }

    pub fn triangle(frequency: f32, amplitude: f32) -> Self {
        Self::new(frequency, amplitude, PolyBlepWaveform::Triangle)
    }

    fn poly_blep(t: f32, dt: f32) -> f32 {
        if dt <= f32::EPSILON {
            return 0.0;
        }

        if t < dt {
            let x = t / dt;
            x + x - x * x - 1.0
        } else if t > 1.0 - dt {
            let x = (t - 1.0) / dt;
            x * x + x + x + 1.0
        } else {
            0.0
        }
    }

    fn poly_blamp(t: f32, dt: f32) -> f32 {
        if dt <= f32::EPSILON {
            return 0.0;
        }

        if t < dt {
            let x = t / dt - 1.0;
            -(x * x * x) / 3.0
        } else if t > 1.0 - dt {
            let x = (t - 1.0) / dt + 1.0;
            (x * x * x) / 3.0
        } else {
            0.0
        }
    }

    fn wrap_phase(phase: f32) -> f32 {
        phase.rem_euclid(1.0)
    }
}

impl SignalProcessor for PolyBlepOscillator {
    fn process(&mut self, sample_rate: f32, inputs: &[f32]) -> f32 {
        let phase_mod = self.get_phase(inputs);
        let freq_mod = self.get_frequency_mod(inputs);
        let freq_override = self.get_frequency(inputs);
        let amp_mod = self.get_amplitude(inputs);
        let pulse_mod = self.get_pulse_width(inputs);

        let base_freq = if freq_override == 0.0 {
            self.frequency
        } else {
            freq_override
        };

        let frequency = (base_freq * (1.0 + freq_mod)).max(0.0);
        let amplitude = self.amplitude * (1.0 + amp_mod);
        let mut pulse_width = (self.pulse_width + pulse_mod).clamp(0.0001, 0.9999);

        let mut phase = Self::wrap_phase(self.phase + phase_mod);
        let freq_per_sample = frequency / sample_rate.max(f32::EPSILON);
        let dt = freq_per_sample.min(1.0);

        if pulse_width <= 0.0 {
            pulse_width = 0.0001;
        }

        let mut value = if frequency >= sample_rate * 0.25 {
            (phase * TAU).sin()
        } else {
            match self.waveform {
                PolyBlepWaveform::Sine => (phase * TAU).sin(),
                PolyBlepWaveform::Saw => {
                    let mut y = 2.0 * phase - 1.0;
                    y -= Self::poly_blep(phase, dt);
                    y
                }
                PolyBlepWaveform::Square => {
                    let mut y = if phase < pulse_width { 1.0 } else { -1.0 };
                    y += Self::poly_blep(phase, dt);
                    let t = Self::wrap_phase(phase + 1.0 - pulse_width);
                    y -= Self::poly_blep(t, dt);
                    y
                }
                PolyBlepWaveform::Triangle => {
                    let mut y = 4.0 * phase;
                    if y >= 3.0 {
                        y -= 4.0;
                    } else if y > 1.0 {
                        y = 2.0 - y;
                    }

                    let t1 = Self::wrap_phase(phase + 0.25);
                    let t2 = Self::wrap_phase(phase + 0.75);
                    y + 4.0 * dt * (Self::poly_blamp(t1, dt) - Self::poly_blamp(t2, dt))
                }
            }
        };

        value *= amplitude;
        self.output = value;

        phase = Self::wrap_phase(self.phase + freq_per_sample);
        self.phase = phase;

        self.output
    }
}

#[cfg(test)]
mod tests {
    use super::{PolyBlepOscillator, PolyBlepWaveform, SignalProcessor};

    #[test]
    fn test_poly_blep_saw_stays_bounded() {
        let sample_rate = 48_000.0;
        let mut osc = PolyBlepOscillator::saw(440.0, 1.0);
        let mut min = f32::MAX;
        let mut max = f32::MIN;
        let inputs = [0.0; 5];

        for _ in 0..(sample_rate as usize / 10) {
            let value = osc.process(sample_rate, &inputs);
            min = min.min(value);
            max = max.max(value);
        }

        assert!(
            min >= -1.25 && max <= 1.25,
            "saw output out of expected bounds"
        );
    }

    #[test]
    fn test_poly_blep_square_continuity() {
        let sample_rate = 48_000.0;
        let mut osc = PolyBlepOscillator::square(880.0, 0.8);
        let inputs = [0.0; 5];

        let mut previous = osc.process(sample_rate, &inputs);
        for _ in 0..1024 {
            let current = osc.process(sample_rate, &inputs);
            let delta = (current - previous).abs();
            assert!(delta <= 1.6, "square produced discontinuity of {delta}");
            previous = current;
        }
    }

    #[test]
    fn test_poly_blep_triangle_shape() {
        let sample_rate = 48_000.0;
        let mut osc = PolyBlepOscillator::new(220.0, 1.0, PolyBlepWaveform::Triangle);
        let inputs = [0.0; 5];

        let mut samples = [0.0; 4];
        for i in 0..samples.len() {
            samples[i] = osc.process(sample_rate, &inputs);
        }

        assert!(samples[0].abs() < 0.25);
        assert!(samples[1] > samples[0]);
    }
}
