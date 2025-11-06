use crate::{
    InputEndpoint, Node, NodeKey, ProcessingContext, ProcessingNode, SignalProcessor, ValueKey,
};
use std::f32::consts::PI;

/// IIR Lowpass Filter using biquad design with bilinear transformation.
#[derive(Debug, Node)]
pub struct IirLowpass {
    #[input(stream)]
    pub input: f32,      // Direct access!

    #[input]
    cutoff: f32,

    #[input]
    q: f32,

    #[output(stream)]
    pub output: f32,     // Direct access!

    // Biquad coefficients
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,

    // State variables (Direct Form II Transposed)
    v1: f32,
    v2: f32,

    // Parameter update management
    frame_counter: usize,
    frames_per_update: usize,
}

impl Default for IirLowpass {
    fn default() -> Self {
        Self {
            input: 0.0,
            cutoff: 1000.0,
            q: std::f32::consts::FRAC_1_SQRT_2,
            output: 0.0,
            b0: 1.0,
            b1: 0.0,
            b2: 0.0,
            a1: 0.0,
            a2: 0.0,
            v1: 0.0,
            v2: 0.0,
            frame_counter: 0,
            frames_per_update: 32,
        }
    }
}

impl IirLowpass {
    pub fn new(cutoff: f32, q: f32) -> Self {
        Self {
            cutoff,
            q,
            ..Default::default()
        }
    }

    fn update_coefficients(&mut self, sample_rate: f32) {
        let nyquist = sample_rate * 0.5 - f32::EPSILON;
        let freq = self.cutoff.clamp(20.0, nyquist);
        let q = self.q.max(0.01);

        let n = 1.0 / (PI * freq / sample_rate).tan();
        let n_squared = n * n;
        let c1 = 1.0 / (1.0 + 1.0 / q * n + n_squared);

        self.b0 = c1;
        self.b1 = c1 * 2.0;
        self.b2 = c1;
        self.a1 = c1 * 2.0 * (1.0 - n_squared);
        self.a2 = c1 * (1.0 - 1.0 / q * n + n_squared);
    }

    #[inline]
    fn process_sample(&mut self, input: f32) -> f32 {
        const DENORMAL_THRESHOLD: f32 = 1e-15;
        let input = if input.abs() < DENORMAL_THRESHOLD {
            0.0
        } else {
            input
        };

        let output = self.b0 * input + self.v1;
        self.v1 = self.b1 * input - self.a1 * output + self.v2;
        self.v2 = self.b2 * input - self.a2 * output;

        if self.v1.abs() < DENORMAL_THRESHOLD {
            self.v1 = 0.0;
        }
        if self.v2.abs() < DENORMAL_THRESHOLD {
            self.v2 = 0.0;
        }

        output
    }

    fn apply_parameter_updates(&mut self, sample_rate: f32, cutoff_in: f32, q_in: f32) {
        if self.frame_counter == 0 {
            let nyquist = sample_rate * 0.5 - f32::EPSILON;
            let cutoff = cutoff_in.clamp(20.0, nyquist);
            let q = q_in.clamp(0.01, 100.0);

            if (cutoff - self.cutoff).abs() > f32::EPSILON
                || (q - self.q).abs() > f32::EPSILON
            {
                self.cutoff = cutoff;
                self.q = q;
                self.update_coefficients(sample_rate);
            }
        }

        self.frame_counter = (self.frame_counter + 1) % self.frames_per_update;
    }

    pub fn reset(&mut self) {
        self.v1 = 0.0;
        self.v2 = 0.0;
    }

    /// Core processing logic
    #[inline]
    pub fn process(&mut self, _sample_rate: f32) -> f32 {
        self.output = self.process_sample(self.input);
        self.output
    }
}

impl SignalProcessor for IirLowpass {
    fn init(&mut self, sample_rate: f32) {
        self.update_coefficients(sample_rate);
    }

    fn process<'a>(&mut self, sample_rate: f32, context: &mut ProcessingContext<'a>) -> f32 {
        self.input = self.get_input(context);

        let cutoff = self.get_cutoff(context);
        let q = self.get_q(context);

        self.apply_parameter_updates(sample_rate, cutoff, q);

        IirLowpass::process(self, sample_rate)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::types::{EventInstance, ValueData};
    use crate::graph::ProcessingContext;

    const EPSILON: f32 = 1e-6;

    fn approx_eq(a: f32, b: f32, epsilon: f32) -> bool {
        (a - b).abs() <= epsilon
    }

    #[test]
    fn test_coefficient_generation_matches_juce() {
        let mut filter = IirLowpass::new(1000.0, std::f32::consts::FRAC_1_SQRT_2);
        let sample_rate = 48_000.0;

        filter.init(sample_rate);

        let freq = 1000.0;
        let q = std::f32::consts::FRAC_1_SQRT_2;
        let n = 1.0 / (PI * freq / sample_rate).tan();
        let n_squared = n * n;
        let c1 = 1.0 / (1.0 + 1.0 / q * n + n_squared);

        let expected_b0 = c1;
        let expected_b1 = c1 * 2.0;
        let expected_b2 = c1;
        let expected_a1 = c1 * 2.0 * (1.0 - n_squared);
        let expected_a2 = c1 * (1.0 - 1.0 / q * n + n_squared);

        assert!(
            approx_eq(filter.b0, expected_b0, EPSILON),
            "b0 mismatch: got {}, expected {}",
            filter.b0,
            expected_b0
        );
        assert!(
            approx_eq(filter.b1, expected_b1, EPSILON),
            "b1 mismatch: got {}, expected {}",
            filter.b1,
            expected_b1
        );
        assert!(
            approx_eq(filter.b2, expected_b2, EPSILON),
            "b2 mismatch: got {}, expected {}",
            filter.b2,
            expected_b2
        );
        assert!(
            approx_eq(filter.a1, expected_a1, EPSILON),
            "a1 mismatch: got {}, expected {}",
            filter.a1,
            expected_a1
        );
        assert!(
            approx_eq(filter.a2, expected_a2, EPSILON),
            "a2 mismatch: got {}, expected {}",
            filter.a2,
            expected_a2
        );
    }

    #[test]
    fn test_dc_gain_is_unity() {
        let mut filter = IirLowpass::new(1000.0, std::f32::consts::FRAC_1_SQRT_2);
        let sample_rate = 48_000.0;
        filter.frames_per_update = 1;
        filter.init(sample_rate);

        let dc_input = 1.0;
        let cutoff = 1000.0;
        let q = std::f32::consts::FRAC_1_SQRT_2;

        let mut output = 0.0;
        for _ in 0..1000 {
            let scalars = vec![dc_input, cutoff, q];
            let value_storage = vec![
                None,
                Some(ValueData::scalar(cutoff)),
                Some(ValueData::scalar(q)),
            ];
            let value_refs: Vec<Option<&ValueData>> =
                value_storage.iter().map(|opt| opt.as_ref()).collect();
            let event_inputs: Vec<&[EventInstance]> = vec![&[]; scalars.len()];
            let mut pending = Vec::new();
            let mut context =
                ProcessingContext::new(&scalars, &value_refs, &event_inputs, &mut pending);
            output = SignalProcessor::process(&mut filter, sample_rate, &mut context);
        }

        assert!(
            approx_eq(output, 1.0, 0.01),
            "DC gain should be ~1.0, got {}",
            output
        );
    }

    #[test]
    fn test_impulse_response() {
        let mut filter = IirLowpass::new(2000.0, std::f32::consts::FRAC_1_SQRT_2);
        let sample_rate = 48_000.0;
        filter.frames_per_update = 1;
        filter.init(sample_rate);

        let cutoff = 2000.0;
        let q = std::f32::consts::FRAC_1_SQRT_2;
        let mut outputs = Vec::new();

        for n in 0..8 {
            let input = if n == 0 { 1.0 } else { 0.0 };
            let scalars = vec![input, cutoff, q];
            let value_storage = vec![
                None,
                Some(ValueData::scalar(cutoff)),
                Some(ValueData::scalar(q)),
            ];
            let value_refs: Vec<Option<&ValueData>> =
                value_storage.iter().map(|opt| opt.as_ref()).collect();
            let event_inputs: Vec<&[EventInstance]> = vec![&[]; scalars.len()];
            let mut pending = Vec::new();
            let mut context =
                ProcessingContext::new(&scalars, &value_refs, &event_inputs, &mut pending);
            outputs.push(SignalProcessor::process(&mut filter, sample_rate, &mut context));
        }

        assert!(outputs[0] > 0.0, "First impulse response should be positive");

        for i in 1..outputs.len() {
            assert!(
                outputs[i].abs() < 2.0,
                "Output {} too large: {}",
                i,
                outputs[i]
            );
        }
    }

    #[test]
    fn test_stability_with_high_q() {
        let mut filter = IirLowpass::new(1000.0, 10.0);
        let sample_rate = 48_000.0;
        filter.frames_per_update = 1;
        filter.init(sample_rate);

        let cutoff = 1000.0;
        let q = 10.0;

        for n in 0..100 {
            let input = if n == 0 { 1.0 } else { 0.0 };
            let scalars = vec![input, cutoff, q];
            let value_storage = vec![
                None,
                Some(ValueData::scalar(cutoff)),
                Some(ValueData::scalar(q)),
            ];
            let value_refs: Vec<Option<&ValueData>> =
                value_storage.iter().map(|opt| opt.as_ref()).collect();
            let event_inputs: Vec<&[EventInstance]> = vec![&[]; scalars.len()];
            let mut pending = Vec::new();
            let mut context =
                ProcessingContext::new(&scalars, &value_refs, &event_inputs, &mut pending);
            let output = SignalProcessor::process(&mut filter, sample_rate, &mut context);

            assert!(
                output.abs() < 10.0,
                "Output unstable at sample {}: {}",
                n,
                output
            );
        }
    }

    #[test]
    fn test_denormal_protection() {
        let mut filter = IirLowpass::new(100.0, std::f32::consts::FRAC_1_SQRT_2);
        let sample_rate = 48_000.0;
        filter.init(sample_rate);

        let tiny_input = 1e-20_f32;
        let output = filter.process_sample(tiny_input);

        assert_eq!(output, 0.0, "Denormal input should be snapped to zero");
    }
}
