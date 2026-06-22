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
        let nyquist = sample_rate * 0.5 - f32::EPSILON;
        let freq = cutoff.clamp(20.0, nyquist);
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
        let nyquist = sample_rate * 0.5 - f32::EPSILON;
        let max_cutoff = nyquist.min(20_000.0);
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

        self.z[0] = high * self.g + band;
        self.z[1] = band * self.g + low;

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
