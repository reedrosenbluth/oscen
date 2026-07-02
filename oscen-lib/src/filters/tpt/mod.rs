use crate::frame::AudioFrame;
use crate::graph::SampleRate;
use crate::{Node, SignalProcessor};
use std::f32::consts::PI;

/// Topology-preserving state-variable lowpass, generic over the frame type `F`
/// (mono `f32` by default, `Frame<N>` for multi-channel). The coefficients are
/// scalar and shared across channels; only the audio path (`input`/`output`)
/// and the integrator state carry one value per channel, so every channel is
/// filtered independently with the same cutoff/Q/modulation.
#[derive(Debug, Node)]
pub struct TptFilter<F: AudioFrame = f32> {
    #[input(stream)]
    pub input: F,
    #[input(stream)]
    pub cutoff: f32,
    #[input(value)]
    pub q: f32,
    #[input(stream)]
    pub f_mod: f32,

    #[output(stream)]
    pub output: F,

    // last applied, sanitized parameters
    current_cutoff: f32,
    current_q: f32,

    // per-channel integrator state
    z: [F; 2],

    // coefficients (scalar, shared across channels)
    h: f32,
    g: f32,
    r: f32,
    k: f32,

    sample_rate: SampleRate,
}

/// These filters are based on the designs outlined in The Art of VA Filter Design
/// by Vadim Zavalishin, with help from Will Pirkle in Virtual Analog Filter Implementation.
/// The topology-preserving transform approach leads to designs where parameter
/// modulation can be applied with minimal instability. Coefficients are recomputed
/// every sample when inputs change.
impl<F: AudioFrame> TptFilter<F> {
    pub fn new(cutoff: f32, q: f32) -> Self {
        let mut filter = Self {
            input: F::default(),
            cutoff,
            q,
            f_mod: 0.0,
            output: F::default(),
            current_cutoff: cutoff,
            current_q: q,
            z: [F::default(); 2],
            h: 0.0,
            g: 0.0,
            r: 0.0,
            k: 0.0,
            sample_rate: SampleRate::default(),
        };
        // Initialize coefficients with default sample rate
        // Will be updated again in init() with actual sample rate
        filter.update_coefficients(44100.0, cutoff, q);
        filter
    }

    fn update_coefficients(&mut self, sample_rate: f32, cutoff: f32, q: f32) {
        // Keep the cutoff strictly below Nyquist with a relative margin: an
        // absolute epsilon rounds away at these magnitudes, letting tan() blow
        // up and degenerate the coefficients.
        let freq = cutoff.clamp(20.0, sample_rate * 0.49);
        let period = 0.5 / sample_rate;
        let f = (2.0 * sample_rate) * (2.0 * PI * freq * period).tan() * period;
        let inv_q = 1.0 / q;

        self.h = 1.0 / (1.0 + inv_q * f + f * f);
        self.g = f;
        self.r = inv_q;
        self.k = self.g + self.r;
        self.current_cutoff = cutoff;
        self.current_q = q;
    }

    #[inline(always)]
    fn apply_parameter_updates(&mut self, sample_rate: f32) {
        let max_cutoff = (sample_rate * 0.49).min(20_000.0);
        let cutoff_base = self.cutoff.clamp(20.0, max_cutoff);
        let q = self.q.clamp(0.1, 10.0);

        let modulation = self.f_mod.clamp(-1.0, 1.0);
        let min_factor = 20.0 / cutoff_base;
        let max_factor = max_cutoff / cutoff_base;
        let factor = (1.0 + modulation).clamp(min_factor, max_factor);
        let cutoff = (cutoff_base * factor).clamp(20.0, max_cutoff);

        if (cutoff - self.current_cutoff).abs() > f32::EPSILON
            || (q - self.current_q).abs() > f32::EPSILON
        {
            self.update_coefficients(sample_rate, cutoff, q);
        }
    }
}

impl<F: AudioFrame> TptFilter<F> {
    /// DSP processing - inputs are already in self fields, write output to self.output
    #[inline(always)]
    pub fn process_internal(&mut self) {
        // Update parameters
        self.apply_parameter_updates(*self.sample_rate);

        // Process (state-variable filter). Coefficients are scalar and the
        // frame arithmetic is element-wise, so each channel runs independently.
        let high = (self.input - self.z[0] * self.k - self.z[1]) * self.h;
        let band = high * self.g + self.z[0];
        let low = band * self.g + self.z[1];

        // Flush the feedback state to zero before it decays into subnormals,
        // which are 10-100x slower on hardware without flush-to-zero.
        self.z[0] = (high * self.g + band).flush_denormal(1e-30);
        self.z[1] = (band * self.g + low).flush_denormal(1e-30);

        // Write output
        self.output = low;
    }
}

// SignalProcessor must be manually implemented
// The Node macro generates ProcessingNode trait and event handler methods
impl<F: AudioFrame> SignalProcessor for TptFilter<F> {
    fn prepare(&mut self) {
        self.update_coefficients(*self.sample_rate, self.cutoff, self.q);
    }

    #[inline(always)]
    fn process(&mut self) {
        // Call our custom process method
        self.process_internal();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::Frame;

    const EPSILON: f32 = 1e-6;

    fn approx_eq(a: f32, b: f32) -> bool {
        (a - b).abs() <= EPSILON
    }

    /// The mono impulse response, reused as the per-channel reference below.
    const IMPULSE_RESPONSE: [f32; 8] = [
        0.014401104,
        0.052318562,
        0.089890145,
        0.11065749,
        0.11862421,
        0.11729243,
        0.10961619,
        0.098000914,
    ];

    /// A stereo filter processes each channel with an independent integrator
    /// state: an impulse on channel 0 only reproduces the mono response on
    /// channel 0 and leaves channel 1 silent (no cross-channel bleed).
    #[test]
    fn stereo_channels_are_independent() {
        let mut filter = TptFilter::<Frame<2>>::new(2_000.0, 0.707);
        let sample_rate = 48_000.0;
        filter.set_sample_rate(sample_rate);
        filter.prepare();

        filter.cutoff = 2_000.0;
        filter.q = 0.707;
        filter.f_mod = 0.0;

        for (n, &expected) in IMPULSE_RESPONSE.iter().enumerate() {
            filter.input = if n == 0 {
                Frame([1.0, 0.0])
            } else {
                Frame([0.0, 0.0])
            };
            filter.process();
            assert!(
                approx_eq(filter.output.0[0], expected),
                "channel 0 mismatch at sample {}: got {}, expected {}",
                n,
                filter.output.0[0],
                expected
            );
            assert!(
                approx_eq(filter.output.0[1], 0.0),
                "channel 1 should stay silent at sample {}: got {}",
                n,
                filter.output.0[1]
            );
        }
    }

    /// Deterministic white-ish noise in [-1, 1) from a seeded LCG.
    fn lcg_noise(seed: &mut u32) -> f32 {
        *seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (*seed >> 8) as f32 / (1 << 24) as f32 * 2.0 - 1.0
    }

    #[test]
    fn test_output_finite_and_bounded_across_extremes() {
        let sample_rates = [22_050.0, 32_000.0, 44_100.0, 48_000.0, 96_000.0];
        let qs = [0.1, 0.707, 10.0];
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
                    let mut filter = TptFilter::<f32>::new(cutoff, q);
                    filter.set_sample_rate(sample_rate);
                    filter.prepare();
                    filter.cutoff = cutoff;
                    filter.q = q;
                    filter.f_mod = 0.0;

                    // The degenerate-coefficient blow-up is a slow exponential,
                    // so drive the filter long enough to expose it.
                    let mut seed = 0x1234_5678_u32;
                    for n in 0..100_000 {
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
            let mut filter = TptFilter::<f32>::new(1_000.0, 0.707);
            filter.set_sample_rate(sample_rate);
            filter.prepare();
            filter.cutoff = 1_000.0;
            filter.q = 0.707;
            filter.f_mod = 0.0;

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

    /// After the input goes silent the integrator state must be flushed to
    /// zero before it decays into the f32 subnormal range, where multiplies
    /// are 10-100x slower on hardware without flush-to-zero.
    #[test]
    fn test_integrator_state_never_goes_subnormal_after_silence() {
        let mut filter = TptFilter::<f32>::new(1_000.0, 0.707);
        filter.set_sample_rate(48_000.0);
        filter.prepare();
        filter.cutoff = 1_000.0;
        filter.q = 0.707;
        filter.f_mod = 0.0;

        filter.input = 1.0;
        filter.process();

        filter.input = 0.0;
        for n in 0..20_000 {
            filter.process();
            for (i, &z) in filter.z.iter().enumerate() {
                assert!(
                    z == 0.0 || z.is_normal(),
                    "integrator state z[{}] is subnormal at sample {}: {:e}",
                    i,
                    n,
                    z
                );
            }
        }
        assert_eq!(filter.z, [0.0, 0.0], "state should settle to exactly zero");
    }

    #[test]
    fn test_coefficients_follow_zavalishin_formulation() {
        let mut filter = TptFilter::<f32>::new(2_000.0, 0.707);
        let sample_rate = 48_000.0;

        filter.set_sample_rate(sample_rate);
        filter.prepare();

        let period = 0.5 / sample_rate;
        let freq = filter.current_cutoff;
        let f = (2.0 * sample_rate) * (2.0 * PI * freq * period).tan() * period;
        let r = 1.0 / filter.current_q;
        let expected_d = 1.0 / (1.0 + r * f + f * f);

        assert!(approx_eq(filter.g, f), "expected g to equal tan transform");
        assert!(
            approx_eq(filter.h, expected_d),
            "expected h coefficient to match ZDF form"
        );
        assert!(approx_eq(filter.r, r), "expected feedback gain to be 1/Q");
        assert!(
            approx_eq(filter.k, filter.g + filter.r),
            "expected k to equal g + 1/Q"
        );
    }

    #[test]
    fn test_impulse_response_matches_reference() {
        let mut filter = TptFilter::<f32>::new(2_000.0, 0.707);
        let sample_rate = 48_000.0;
        filter.set_sample_rate(sample_rate);
        filter.prepare();

        filter.cutoff = 2_000.0;
        filter.q = 0.707;
        filter.f_mod = 0.0;
        let mut outputs = Vec::new();

        for n in 0..8 {
            filter.input = if n == 0 { 1.0 } else { 0.0 };
            filter.process();
            outputs.push(filter.output);
        }

        let expected = [
            0.014401104,
            0.052318562,
            0.089890145,
            0.11065749,
            0.11862421,
            0.11729243,
            0.10961619,
            0.098000914,
        ];

        for (i, (&actual, &target)) in outputs.iter().zip(expected.iter()).enumerate() {
            assert!(
                approx_eq(actual, target),
                "output mismatch at sample {}: got {}, expected {}",
                i,
                actual,
                target
            );
        }
    }
}
