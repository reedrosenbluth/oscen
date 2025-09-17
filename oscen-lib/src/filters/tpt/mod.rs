use crate::{
    EndpointType, InputEndpoint, Node, NodeKey, OutputEndpoint, ProcessingNode, SignalProcessor,
    ValueKey,
};
use std::f32::consts::PI;

#[derive(Debug, Default, Node)]
pub struct TptFilter {
    #[input(stream)]
    input: f32,
    #[input]
    cutoff: f32,
    #[input]
    q: f32,

    #[output(stream)]
    output: f32,

    // state
    z: [f32; 2],

    // coefficients
    h: f32,
    g: f32,
    r: f32,
    k: f32,

    // frame counting
    frame_counter: usize,
    frames_per_update: usize,
}

/// These filters are based on the designs outlined in The Art of VA Filter Design
/// by Vadim Zavalishin, with help from Will Pirkle in Virtual Analog Filter Implementation.
/// The topology-preserving transform approach leads to designs where parameter
/// modulation can be applied with minimal instability.
///
/// Parameter changes are applied at a lower rate than processor.frequency to reduce
/// computational cost, and the frames between updates can be altered using the
/// `framesPerParameterUpdate`, smaller numbers causing more frequent updates.
impl TptFilter {
    pub fn new(cutoff: f32, q: f32) -> Self {
        Self {
            cutoff,
            q,
            frames_per_update: 32,
            ..Default::default()
        }
    }

    fn update_coefficients(&mut self, sample_rate: f32) {
        let nyquist = sample_rate * 0.5 - f32::EPSILON;
        let freq = self.cutoff.clamp(20.0, nyquist);
        let period = 0.5 / sample_rate;
        let f = (2.0 * sample_rate) * (2.0 * PI * freq * period).tan() * period;
        let inv_q = 1.0 / self.q;

        self.h = 1.0 / (1.0 + inv_q * f + f * f);
        self.g = f;
        self.r = inv_q;
        self.k = self.g + self.r;
    }
}

impl SignalProcessor for TptFilter {
    fn init(&mut self, sample_rate: f32) {
        self.update_coefficients(sample_rate);
    }

    fn process(&mut self, sample_rate: f32, inputs: &[f32]) -> f32 {
        let input = self.get_input(inputs);

        if self.frame_counter == 0 {
            let nyquist = sample_rate * 0.5 - f32::EPSILON;
            let cutoff = self.get_cutoff(inputs).clamp(20.0, nyquist);
            let q = self.get_q(inputs).clamp(0.1, 10.0);

            if cutoff != self.cutoff || q != self.q {
                self.cutoff = cutoff;
                self.q = q;
                self.update_coefficients(sample_rate);
            }
        }

        self.frame_counter = (self.frame_counter + 1) % self.frames_per_update;

        let high = (input - self.k * self.z[0] - self.z[1]) * self.h;
        let band = self.g * high + self.z[0];
        let low = self.g * band + self.z[1];

        self.z[0] = self.g * high + band;
        self.z[1] = self.g * band + low;

        self.output = low;
        self.output
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EPSILON: f32 = 1e-6;

    fn approx_eq(a: f32, b: f32) -> bool {
        (a - b).abs() <= EPSILON
    }

    #[test]
    fn test_coefficients_follow_zavalishin_formulation() {
        let mut filter = TptFilter::new(2_000.0, 0.707);
        let sample_rate = 48_000.0;

        filter.init(sample_rate);

        let period = 0.5 / sample_rate;
        let freq = filter.cutoff;
        let f = (2.0 * sample_rate) * (2.0 * PI * freq * period).tan() * period;
        let r = 1.0 / filter.q;
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
        let mut filter = TptFilter::new(2_000.0, 0.707);
        let sample_rate = 48_000.0;
        filter.frames_per_update = 1;
        filter.init(sample_rate);

        let cutoff = 2_000.0;
        let q = 0.707;
        let mut outputs = Vec::new();

        for n in 0..8 {
            let input = if n == 0 { 1.0 } else { 0.0 };
            outputs.push(filter.process(sample_rate, &[input, cutoff, q]));
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
