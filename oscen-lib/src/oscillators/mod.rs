use crate::{
    InputEndpoint, Node, NodeKey, ProcessingContext, ProcessingNode,
    SignalProcessor, ValueKey,
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
    pub frequency_mod: f32,  // PUBLIC for CMajor-style access
    #[input(value)]
    amplitude: f32,

    #[output(stream)]
    pub output: f32,  // PUBLIC for CMajor-style access

    waveform: fn(f32) -> f32,
}

impl Oscillator {
    /// User-defined processing logic
    pub fn process_dsp(&mut self, sample_rate: f32) {
        let frequency = self.frequency * (1.0 + self.frequency_mod);
        let amplitude = self.amplitude;

        let modulated_phase = self.phase % 1.0;
        self.output = (self.waveform)(modulated_phase) * amplitude;

        self.phase += frequency / sample_rate;
        self.phase %= 1.0;
    }

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
    /// Auto-populated implementation (TODO: auto-generate via macro)
    fn process<'a>(&mut self, sample_rate: f32, context: &mut ProcessingContext<'a>) {
        // Populate stream inputs from context
        self.frequency_mod = context.stream(0);

        // Populate value inputs from context
        if let Some(value_ref) = context.value(0) {
            if let Some(scalar) = value_ref.as_scalar() {
                self.phase = scalar;
            }
        }
        if let Some(value_ref) = context.value(1) {
            if let Some(scalar) = value_ref.as_scalar() {
                self.frequency = scalar;
            }
        }
        if let Some(value_ref) = context.value(2) {
            if let Some(scalar) = value_ref.as_scalar() {
                self.amplitude = scalar;
            }
        }

        // Call user's process method
        self.process_dsp(sample_rate);

        // Output is now in self.output - runtime graph reads it via get_stream_output()
    }

    // Accessor methods for runtime graph routing
    fn get_stream_output(&self, index: usize) -> Option<f32> {
        match index {
            0 => Some(self.output),
            _ => None,
        }
    }

    fn set_stream_input(&mut self, index: usize, value: f32) {
        match index {
            0 => self.frequency_mod = value,
            _ => {}
        }
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
    #[input(stream)]
    pub phase_mod: f32,  // PUBLIC
    #[input(value)]
    frequency: f32,
    #[input(stream)]
    pub frequency_mod: f32,  // PUBLIC
    #[input(value)]
    amplitude: f32,
    #[input(value)]
    pulse_width: f32,

    #[output(stream)]
    pub output: f32,  // PUBLIC

    waveform: PolyBlepWaveform,
}

impl PolyBlepOscillator {
    pub fn new(frequency: f32, amplitude: f32, waveform: PolyBlepWaveform) -> Self {
        Self {
            phase: 0.0,
            phase_mod: 0.0,
            frequency,
            frequency_mod: 0.0,
            amplitude,
            pulse_width: 0.5,
            output: 0.0,
            waveform,
        }
    }

    // Accessor methods for runtime graph (TODO: auto-generate via macro)
    pub fn get_stream_output(&self, index: usize) -> Option<f32> {
        match index {
            0 => Some(self.output),
            _ => None,
        }
    }

    pub fn set_stream_input(&mut self, index: usize, value: f32) {
        match index {
            0 => self.phase_mod = value,
            1 => self.frequency_mod = value,
            _ => {}
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

    fn process_internal(
        &mut self,
        sample_rate: f32,
        phase_offset: f32,
        phase_mod_stream: f32,
        freq_mod: f32,
        freq_input: f32,
        amp_mod: f32,
        pulse_mod: f32,
    ) -> f32 {
        let base_freq = if freq_input == 0.0 {
            self.frequency
        } else {
            freq_input
        };
        let frequency = (base_freq * (1.0 + freq_mod)).max(0.0);
        let amplitude = self.amplitude * (1.0 + amp_mod);
        let mut pulse_width = (self.pulse_width + pulse_mod).clamp(0.0001, 0.9999);

        let mut phase = Self::wrap_phase(self.phase + phase_offset + phase_mod_stream);
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

impl SignalProcessor for PolyBlepOscillator {
    /// Process with CMajor-style direct field access
    fn process<'a>(
        &mut self,
        sample_rate: f32,
        context: &mut ProcessingContext<'a>,
    ) {
        // Populate stream inputs from context
        self.phase_mod = context.stream(0);
        self.frequency_mod = context.stream(1);

        // Populate value inputs from context
        if let Some(value_ref) = context.value(0) {
            if let Some(scalar) = value_ref.as_scalar() {
                self.phase = scalar;
            }
        }
        if let Some(value_ref) = context.value(1) {
            if let Some(scalar) = value_ref.as_scalar() {
                self.frequency = scalar;
            }
        }
        if let Some(value_ref) = context.value(2) {
            if let Some(scalar) = value_ref.as_scalar() {
                self.amplitude = scalar;
            }
        }
        if let Some(value_ref) = context.value(3) {
            if let Some(scalar) = value_ref.as_scalar() {
                self.pulse_width = scalar;
            }
        }

        // Call internal processing (TODO: simplify this later)
        self.output = self.process_internal(
            sample_rate,
            0.0,  // phase_offset from value context
            self.phase_mod,
            self.frequency_mod,
            self.frequency,
            self.amplitude,
            self.pulse_width,
        );
    }

    // Accessor methods for runtime graph routing
    fn get_stream_output(&self, index: usize) -> Option<f32> {
        match index {
            0 => Some(self.output),
            _ => None,
        }
    }

    fn set_stream_input(&mut self, index: usize, value: f32) {
        match index {
            0 => self.phase_mod = value,
            1 => self.frequency_mod = value,
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{PolyBlepOscillator, PolyBlepWaveform, SignalProcessor};
    use crate::graph::types::{EventInstance, ValueData};
    use crate::graph::{IOStructAccess, PendingEvent, ProcessingContext, ProcessingNode};

    #[test]
    fn test_poly_blep_saw_stays_bounded() {
        let sample_rate = 48_000.0;
        let mut osc = PolyBlepOscillator::saw(440.0, 1.0);
        let mut min = f32::MAX;
        let mut max = f32::MIN;
        let value_template: Vec<Option<ValueData>> = vec![
            Some(ValueData::scalar(0.0)), // phase
            None,                         // phase_mod (stream)
            Some(ValueData::scalar(0.0)), // frequency override
            None,                         // frequency mod (stream)
            Some(ValueData::scalar(0.0)), // amplitude mod
            Some(ValueData::scalar(0.0)), // pulse width mod
        ];

        for _ in 0..(sample_rate as usize / 10) {
            let scalars = vec![0.0, 0.0, 0.0, 0.0, 0.0, 0.0];
            let value_storage = value_template.clone();
            let value_refs: Vec<Option<&ValueData>> =
                value_storage.iter().map(|opt| opt.as_ref()).collect();
            let event_inputs: Vec<&[EventInstance]> = vec![&[]; scalars.len()];
            let mut pending = Vec::<PendingEvent>::new();
            let mut context =
                ProcessingContext::new(&scalars, &value_refs, &event_inputs, &mut pending);

            osc.process(sample_rate, &mut context);
            let value = osc.output;
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
        let value_template: Vec<Option<ValueData>> = vec![
            Some(ValueData::scalar(0.0)),
            None,
            Some(ValueData::scalar(0.0)),
            None,
            Some(ValueData::scalar(0.0)),
            Some(ValueData::scalar(0.0)),
        ];

        let mut previous = {
            let scalars = vec![0.0, 0.0, 0.0, 0.0, 0.0, 0.0];
            let value_storage = value_template.clone();
            let value_refs: Vec<Option<&ValueData>> =
                value_storage.iter().map(|opt| opt.as_ref()).collect();
            let event_inputs: Vec<&[EventInstance]> = vec![&[]; scalars.len()];
            let mut pending = Vec::<PendingEvent>::new();
            let mut context =
                ProcessingContext::new(&scalars, &value_refs, &event_inputs, &mut pending);
            osc.process(sample_rate, &mut context);
            osc.output
        };
        for _ in 0..1024 {
            let scalars = vec![0.0, 0.0, 0.0, 0.0, 0.0, 0.0];
            let value_storage = value_template.clone();
            let value_refs: Vec<Option<&ValueData>> =
                value_storage.iter().map(|opt| opt.as_ref()).collect();
            let event_inputs: Vec<&[EventInstance]> = vec![&[]; scalars.len()];
            let mut pending = Vec::<PendingEvent>::new();
            let mut context =
                ProcessingContext::new(&scalars, &value_refs, &event_inputs, &mut pending);

            osc.process(sample_rate, &mut context);
            let current = osc.output;
            let delta = (current - previous).abs();
            assert!(delta <= 1.6, "square produced discontinuity of {delta}");
            previous = current;
        }
    }

    #[test]
    fn test_poly_blep_triangle_shape() {
        let sample_rate = 48_000.0;
        let mut osc = PolyBlepOscillator::new(220.0, 1.0, PolyBlepWaveform::Triangle);
        // Initialize stream input fields before calling process()
        osc.phase_mod = 0.0;
        osc.frequency_mod = 0.0;

        let mut samples = [0.0; 4];
        for i in 0..samples.len() {
            osc.process(sample_rate, &mut ProcessingContext::new(&[], &[], &[], &mut Vec::new()));
            samples[i] = osc.output;
        }

        assert!(samples[0].abs() < 0.25);
        assert!(samples[1] > samples[0]);
    }
}
