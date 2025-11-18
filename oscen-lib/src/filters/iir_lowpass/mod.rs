use crate::{InputEndpoint, Node, NodeKey, ProcessingNode, SignalProcessor, ValueKey};
use std::f32::consts::PI;

/// IIR Lowpass Filter using biquad design with bilinear transformation.
///
/// This implementation follows JUCE's IIR filter design, using:
/// - Bilinear transform with pre-warping for coefficient generation
/// - Direct Form II Transposed structure for efficient processing
/// - Default Q of 1/√2 (0.707) for Butterworth response
///
/// The biquad transfer function is:
/// H(z) = (b0 + b1*z^-1 + b2*z^-2) / (1 + a1*z^-1 + a2*z^-2)
#[derive(Debug, Node)]
pub struct IirLowpass {
    #[input(stream)]
    pub input: f32,
    #[input]
    cutoff: f32,
    #[input]
    q: f32,

    #[output(stream)]
    pub output: f32,

    // Biquad coefficients
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,

    // State variables (Direct Form II Transposed)
    v1: f32,
    v2: f32,

    sample_rate: f32,
    // Parameter update management
    frame_counter: usize,
    frames_per_update: usize,
}

impl Default for IirLowpass {
    fn default() -> Self {
        Self {
            input: 0.0,
            cutoff: 1000.0,
            q: std::f32::consts::FRAC_1_SQRT_2, // 0.707 for Butterworth response
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
            sample_rate: 44100.0,        }
    }
}

impl IirLowpass {
    /// Create a new IIR lowpass filter with specified cutoff frequency and Q.
    ///
    /// # Arguments
    /// * `cutoff` - Cutoff frequency in Hz
    /// * `q` - Quality factor (default 0.707 for Butterworth response)
    pub fn new(cutoff: f32, q: f32) -> Self {
        Self {
            cutoff,
            q,
            ..Default::default()
        }
    }

    /// Update biquad coefficients using bilinear transform.
    ///
    /// This implements the JUCE makeLowPass algorithm:
    /// 1. Pre-warp the frequency using tan to account for bilinear transform warping
    /// 2. Calculate coefficients in the analog domain
    /// 3. Apply bilinear transform to get digital coefficients
    fn update_coefficients(&mut self, sample_rate: f32) {
        let nyquist = sample_rate * 0.5 - f32::EPSILON;
        let freq = self.cutoff.clamp(20.0, nyquist);
        let q = self.q.max(0.01); // Prevent division by zero

        // Pre-warping: n = 1/tan(π·f/fs)
        let n = 1.0 / (PI * freq / sample_rate).tan();
        let n_squared = n * n;
        let c1 = 1.0 / (1.0 + 1.0 / q * n + n_squared);

        // Calculate coefficients
        self.b0 = c1;
        self.b1 = c1 * 2.0;
        self.b2 = c1;
        self.a1 = c1 * 2.0 * (1.0 - n_squared);
        self.a2 = c1 * (1.0 - 1.0 / q * n + n_squared);
    }

    /// Process a single sample using Direct Form II Transposed structure.
    ///
    /// This structure is computationally efficient and numerically stable:
    /// - Only 2 state variables needed
    /// - Minimal delay in signal path
    /// - Good numerical properties for fixed-point implementations
    #[inline]
    fn process_sample(&mut self, input: f32) -> f32 {
        // Denormal protection: snap very small values to zero
        const DENORMAL_THRESHOLD: f32 = 1e-15;
        let input = if input.abs() < DENORMAL_THRESHOLD {
            0.0
        } else {
            input
        };

        // Direct Form II Transposed structure
        let output = self.b0 * input + self.v1;
        self.v1 = self.b1 * input - self.a1 * output + self.v2;
        self.v2 = self.b2 * input - self.a2 * output;

        // Denormal protection on state variables
        if self.v1.abs() < DENORMAL_THRESHOLD {
            self.v1 = 0.0;
        }
        if self.v2.abs() < DENORMAL_THRESHOLD {
            self.v2 = 0.0;
        }

        output
    }

    /// Apply parameter updates at a reduced rate to minimize computational cost.
    fn apply_parameter_updates(&mut self, sample_rate: f32, cutoff_in: f32, q_in: f32) {
        if self.frame_counter == 0 {
            let nyquist = sample_rate * 0.5 - f32::EPSILON;
            let cutoff = cutoff_in.clamp(20.0, nyquist);
            let q = q_in.clamp(0.01, 100.0);

            if (cutoff - self.cutoff).abs() > f32::EPSILON || (q - self.q).abs() > f32::EPSILON {
                self.cutoff = cutoff;
                self.q = q;
                self.update_coefficients(self.sample_rate);
            }
        }

        self.frame_counter = (self.frame_counter + 1) % self.frames_per_update;
    }

    /// Reset the filter state (clear delay elements).
    pub fn reset(&mut self) {
        self.v1 = 0.0;
        self.v2 = 0.0;
    }
}

impl SignalProcessor for IirLowpass {
    fn init(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        self.update_coefficients(sample_rate);
    }

    #[inline(always)]
    fn process(&mut self) {
        // Update filter parameters if needed
        self.apply_parameter_updates(self.sample_rate, self.cutoff, self.q);

        // Process sample
        self.output = self.process_sample(self.input);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use arrayvec::ArrayVec;
    use crate::graph::types::{EventInstance, ValueData};
    use crate::graph::{IOStructAccess, NodeIO, ProcessingContext, ProcessingNode};

    const EPSILON: f32 = 1e-6;

    fn approx_eq(a: f32, b: f32, epsilon: f32) -> bool {
        (a - b).abs() <= epsilon
    }

    #[test]
    fn test_coefficient_generation_matches_juce() {
        let mut filter = IirLowpass::new(1000.0, std::f32::consts::FRAC_1_SQRT_2);
        let sample_rate = 48_000.0;

        filter.init(sample_rate);

        // Manually calculate expected coefficients using JUCE formula
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

        // Feed DC signal and check steady-state output
        let dc_input = 1.0;
        let cutoff = 1000.0;
        let q = std::f32::consts::FRAC_1_SQRT_2;

        let mut output = 0.0;
        for _ in 0..1000 {
            let stream_inputs: Vec<ArrayVec<f32, 128>> = vec![dc_input, cutoff, q]
                .into_iter()
                .map(|v| {
                    let mut av = ArrayVec::new();
                    av.push(v);
                    av
                })
                .collect();
            let value_storage = vec![
                None,
                Some(ValueData::scalar(cutoff)),
                Some(ValueData::scalar(q)),
            ];
            let value_refs: Vec<Option<&ValueData>> =
                value_storage.iter().map(|opt| opt.as_ref()).collect();
            let event_inputs: Vec<&[EventInstance]> = vec![&[]; stream_inputs.len()];
            let mut pending = Vec::new();
            let mut context =
                ProcessingContext::new(&stream_inputs, &value_refs, &event_inputs, &mut pending);
            filter.input = dc_input;
            filter.read_inputs(&mut context);
            filter.process(sample_rate);
            output = filter.output;
        }

        // DC gain should be approximately 1.0 for a lowpass filter
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
            let stream_inputs: Vec<ArrayVec<f32, 128>> = vec![input, cutoff, q]
                .into_iter()
                .map(|v| {
                    let mut av = ArrayVec::new();
                    av.push(v);
                    av
                })
                .collect();
            let value_storage = vec![
                None,
                Some(ValueData::scalar(cutoff)),
                Some(ValueData::scalar(q)),
            ];
            let value_refs: Vec<Option<&ValueData>> =
                value_storage.iter().map(|opt| opt.as_ref()).collect();
            let event_inputs: Vec<&[EventInstance]> = vec![&[]; stream_inputs.len()];
            let mut pending = Vec::new();
            let mut context =
                ProcessingContext::new(&stream_inputs, &value_refs, &event_inputs, &mut pending);
            filter.input = input;
            filter.read_inputs(&mut context);
            filter.process(sample_rate);
            outputs.push(filter.output);
        }

        // First output should be positive (impulse response of lowpass)
        assert!(
            outputs[0] > 0.0,
            "First impulse response should be positive"
        );

        // Outputs should decay over time (no instability)
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

        // Process impulse and verify stability
        for n in 0..100 {
            let input = if n == 0 { 1.0 } else { 0.0 };
            let stream_inputs: Vec<ArrayVec<f32, 128>> = vec![input, cutoff, q]
                .into_iter()
                .map(|v| {
                    let mut av = ArrayVec::new();
                    av.push(v);
                    av
                })
                .collect();
            let value_storage = vec![
                None,
                Some(ValueData::scalar(cutoff)),
                Some(ValueData::scalar(q)),
            ];
            let value_refs: Vec<Option<&ValueData>> =
                value_storage.iter().map(|opt| opt.as_ref()).collect();
            let event_inputs: Vec<&[EventInstance]> = vec![&[]; stream_inputs.len()];
            let mut pending = Vec::new();
            let mut context =
                ProcessingContext::new(&stream_inputs, &value_refs, &event_inputs, &mut pending);
            filter.input = input;
            filter.read_inputs(&mut context);
            filter.process(sample_rate);
            let output = filter.output;

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

        // Process very small input
        let tiny_input = 1e-20_f32;
        let output = filter.process_sample(tiny_input);

        // Should return zero due to denormal protection
        assert_eq!(output, 0.0, "Denormal input should be snapped to zero");
    }
}
