use crate::graph::SampleRate;
use crate::{Node, SignalProcessor};
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
    #[input(value)]
    cutoff: f32,
    #[input(value)]
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

    sample_rate: SampleRate,
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
            sample_rate: SampleRate::default(),
        }
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
        // Keep the cutoff strictly below Nyquist with a relative margin: an
        // absolute epsilon rounds away at these magnitudes, letting tan() blow
        // up and push the poles onto the unit circle.
        let freq = self.cutoff.clamp(20.0, sample_rate * 0.49);
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
    fn apply_parameter_updates(&mut self, sample_rate: f32) {
        if self.frame_counter == 0 {
            self.update_coefficients(sample_rate);
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
    fn prepare(&mut self) {
        self.update_coefficients(*self.sample_rate);
    }

    #[inline(always)]
    fn process(&mut self) {
        // Update filter parameters if needed
        self.apply_parameter_updates(*self.sample_rate);

        // Process sample
        let input = self.input;
        let output = self.process_sample(input);
        self.output = output;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EPSILON: f32 = 1e-6;

    fn approx_eq(a: f32, b: f32, epsilon: f32) -> bool {
        (a - b).abs() <= epsilon
    }

    #[test]
    fn test_coefficient_generation_matches_juce() {
        let mut filter = IirLowpass::new(1000.0, std::f32::consts::FRAC_1_SQRT_2);
        let sample_rate = 48_000.0;

        filter.set_sample_rate(sample_rate);
        filter.prepare();

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
        filter.set_sample_rate(sample_rate);
        filter.prepare();

        // Feed DC signal and check steady-state output
        filter.cutoff = 1000.0;
        filter.q = std::f32::consts::FRAC_1_SQRT_2;

        for _ in 0..1000 {
            filter.input = 1.0;
            filter.process();
        }

        // DC gain should be approximately 1.0 for a lowpass filter
        assert!(
            approx_eq(filter.output, 1.0, 0.01),
            "DC gain should be ~1.0, got {}",
            filter.output
        );
    }

    #[test]
    fn test_impulse_response() {
        let mut filter = IirLowpass::new(2000.0, std::f32::consts::FRAC_1_SQRT_2);
        let sample_rate = 48_000.0;
        filter.frames_per_update = 1;
        filter.set_sample_rate(sample_rate);
        filter.prepare();

        filter.cutoff = 2000.0;
        filter.q = std::f32::consts::FRAC_1_SQRT_2;
        let mut outputs = Vec::new();

        for n in 0..8 {
            filter.input = if n == 0 { 1.0 } else { 0.0 };
            filter.process();
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
        filter.set_sample_rate(sample_rate);
        filter.prepare();

        filter.cutoff = 1000.0;
        filter.q = 10.0;

        // Process impulse and verify stability
        for n in 0..100 {
            filter.input = if n == 0 { 1.0 } else { 0.0 };
            filter.process();

            assert!(
                filter.output.abs() < 10.0,
                "Output unstable at sample {}: {}",
                n,
                filter.output
            );
        }
    }

    /// Deterministic white-ish noise in [-1, 1) from a seeded LCG.
    fn lcg_noise(seed: &mut u32) -> f32 {
        *seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (*seed >> 8) as f32 / (1 << 24) as f32 * 2.0 - 1.0
    }

    #[test]
    fn test_coefficients_stable_at_and_beyond_nyquist() {
        // Cutoffs at or above Nyquist must still produce poles strictly inside
        // the unit circle (biquad stability triangle: |a2| < 1, |a1| < 1 + a2).
        for &sample_rate in &[22_050.0, 32_000.0, 44_100.0, 48_000.0, 96_000.0] {
            for &cutoff in &[sample_rate * 0.5, sample_rate, 1e6] {
                let mut filter = IirLowpass::new(cutoff, std::f32::consts::FRAC_1_SQRT_2);
                filter.set_sample_rate(sample_rate);
                filter.prepare();

                assert!(
                    filter.a2.abs() < 1.0,
                    "pole on/outside unit circle at sr={}, cutoff={}: a2={}",
                    sample_rate,
                    cutoff,
                    filter.a2
                );
                assert!(
                    filter.a1.abs() < 1.0 + filter.a2,
                    "unstable coefficients at sr={}, cutoff={}: a1={}, a2={}",
                    sample_rate,
                    cutoff,
                    filter.a1,
                    filter.a2
                );
            }
        }
    }

    #[test]
    fn test_output_finite_and_bounded_across_extremes() {
        let sample_rates = [22_050.0, 32_000.0, 44_100.0, 48_000.0, 96_000.0];
        let qs = [0.01, std::f32::consts::FRAC_1_SQRT_2, 10.0];
        for &sample_rate in &sample_rates {
            let cutoffs = [
                20.0,
                1_000.0,
                sample_rate * 0.25,
                sample_rate * 0.49,
                sample_rate * 0.5,
                sample_rate,
                100_000.0,
            ];
            for &cutoff in &cutoffs {
                for &q in &qs {
                    let mut filter = IirLowpass::new(cutoff, q);
                    filter.set_sample_rate(sample_rate);
                    filter.prepare();

                    let mut seed = 0x1234_5678_u32;
                    for n in 0..8_000 {
                        filter.input = lcg_noise(&mut seed);
                        filter.process();
                        assert!(
                            filter.output.is_finite() && filter.output.abs() < 100.0,
                            "unstable output at sr={}, cutoff={}, q={}, sample {}: {}",
                            sample_rate,
                            cutoff,
                            q,
                            n,
                            filter.output
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn test_dc_gain_unity_across_sample_rates() {
        use float_cmp::approx_eq;

        for &sample_rate in &[22_050.0, 32_000.0, 44_100.0, 48_000.0, 96_000.0] {
            let mut filter = IirLowpass::new(1_000.0, std::f32::consts::FRAC_1_SQRT_2);
            filter.set_sample_rate(sample_rate);
            filter.prepare();

            for _ in 0..4_000 {
                filter.input = 1.0;
                filter.process();
            }

            assert!(
                approx_eq!(f32, filter.output, 1.0, epsilon = 0.01),
                "DC gain should be ~1.0 at sr={}: got {}",
                sample_rate,
                filter.output
            );
        }
    }

    #[test]
    fn test_denormal_protection() {
        let mut filter = IirLowpass::new(100.0, std::f32::consts::FRAC_1_SQRT_2);
        let sample_rate = 48_000.0;
        filter.set_sample_rate(sample_rate);
        filter.prepare();

        // Process very small input
        let tiny_input = 1e-20_f32;
        let output = filter.process_sample(tiny_input);

        // Should return zero due to denormal protection
        assert_eq!(output, 0.0, "Denormal input should be snapped to zero");
    }
}
