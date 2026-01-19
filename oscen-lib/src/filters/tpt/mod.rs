use crate::{InputEndpoint, Node, NodeKey, ProcessingNode, SignalProcessor, ValueKey};
use std::f32::consts::PI;

#[derive(Debug, Node)]
pub struct TptFilter {
    #[input(stream)]
    pub input: f32,
    #[input]
    pub cutoff: f32,
    #[input]
    pub q: f32,
    #[input(stream)]
    pub f_mod: f32,

    #[output(stream)]
    pub output: f32,

    // last applied, sanitized parameters
    current_cutoff: f32,
    current_q: f32,

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

    sample_rate: f32,
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
        let mut filter = Self {
            input: 0.0,
            cutoff,
            q,
            f_mod: 0.0,
            output: 0.0,
            current_cutoff: cutoff,
            current_q: q,
            z: [0.0; 2],
            h: 0.0,
            g: 0.0,
            r: 0.0,
            k: 0.0,
            frame_counter: 0,
            frames_per_update: 32,
            sample_rate: 44100.0,
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

    //TODO: why do we need this function?
    #[inline(always)]
    fn apply_parameter_updates(&mut self, sample_rate: f32, cutoff_in: f32, q_in: f32, f_mod: f32) {
        if self.frame_counter == 0 {
            let nyquist = sample_rate * 0.5 - f32::EPSILON;
            let max_cutoff = nyquist.min(20_000.0);
            let cutoff_base = cutoff_in.clamp(20.0, max_cutoff);
            let q = q_in.clamp(0.1, 10.0);

            let modulation = f_mod.clamp(-1.0, 1.0);
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

        self.frame_counter = (self.frame_counter + 1) % self.frames_per_update;
    }
}

impl TptFilter {
    /// DSP processing - inputs are already in self fields, write output to self.output
    #[inline(always)]
    pub fn process_internal(&mut self) {
        // Update parameters
        self.apply_parameter_updates(self.sample_rate, self.cutoff, self.q, self.f_mod);

        // Process (state-variable filter)
        let high = (self.input - self.k * self.z[0] - self.z[1]) * self.h;
        let band = self.g * high + self.z[0];
        let low = self.g * band + self.z[1];

        self.z[0] = self.g * high + band;
        self.z[1] = self.g * band + low;

        // Write output
        self.output = low;
    }
}

// SignalProcessor must be manually implemented
// The Node macro only generates NodeIO and ProcessingNode traits
impl SignalProcessor for TptFilter {
    fn init(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        self.update_coefficients(sample_rate, self.cutoff, self.q);
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
    use arrayvec::ArrayVec;
    use crate::graph::types::{EventInstance, ValueData};
    use crate::graph::{NodeIO, ProcessingContext, ProcessingNode};

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
        let mut filter = TptFilter::new(2_000.0, 0.707);
        let sample_rate = 48_000.0;
        filter.frames_per_update = 1;
        filter.init(sample_rate);

        let cutoff = 2_000.0;
        let q = 0.707;
        let mut outputs = Vec::new();

        for n in 0..8 {
            let input = if n == 0 { 1.0 } else { 0.0 };
            let stream_inputs: Vec<ArrayVec<f32, 128>> = vec![input, cutoff, q, 0.0]
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
                None,
            ];
            let value_refs: Vec<Option<&ValueData>> =
                value_storage.iter().map(|opt| opt.as_ref()).collect();
            let event_inputs: Vec<&[EventInstance]> = vec![&[]; stream_inputs.len()];
            let mut context =
                ProcessingContext::new(&stream_inputs, &value_refs, &event_inputs);
            filter.input = input;
            filter.f_mod = 0.0;
            filter.read_inputs(&mut context);
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
