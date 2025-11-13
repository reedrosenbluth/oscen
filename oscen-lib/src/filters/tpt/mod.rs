use crate::{
    InputEndpoint, Node, NodeKey, ProcessingContext, ProcessingNode, SignalProcessor, ValueKey,
};
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
    io: TptFilterIO,
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
            input: 0.0,
            cutoff,
            q,
            f_mod: 0.0,
            output: 0.0,
            z: [0.0; 2],
            h: 0.0,
            g: 0.0,
            r: 0.0,
            k: 0.0,
            frame_counter: 0,
            frames_per_update: 32,
            io: TptFilterIO {
                input: 0.0,
                f_mod: 0.0,
                output: 0.0,
            },
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

    //TODO: why do we need this function?
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

            if cutoff != self.cutoff || q != self.q {
                self.cutoff = cutoff;
                self.q = q;
                self.update_coefficients(sample_rate);
            }
        }

        self.frame_counter = (self.frame_counter + 1) % self.frames_per_update;
    }
}

impl SignalProcessor for TptFilter {
    fn init(&mut self, sample_rate: f32) {
        self.update_coefficients(sample_rate);
    }

    /// Process using struct-of-arrays I/O pattern.
    ///
    /// Input and output are accessed via self.input/self.output
    /// Graph pre-populates stream inputs, node writes to output.
    fn process<'a>(
        &mut self,
        sample_rate: f32,
        context: &mut ProcessingContext<'a>,
    ) {
        // Read stream inputs from self (pre-populated by graph)
        let input = self.input;
        let f_mod = self.f_mod;

        // Get value inputs from graph
        let cutoff = self.get_cutoff(context);
        let q = self.get_q(context);

        // Update parameters
        self.apply_parameter_updates(sample_rate, cutoff, q, f_mod);

        // Process
        let high = (input - self.k * self.z[0] - self.z[1]) * self.h;
        let band = self.g * high + self.z[0];
        let low = self.g * band + self.z[1];

        self.z[0] = self.g * high + band;
        self.z[1] = self.g * band + low;

        // Write stream output to self
        self.output = low;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::types::{EventInstance, ValueData};
    use crate::graph::{PendingEvent, ProcessingContext, ProcessingNode};

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
            let scalars = vec![input, cutoff, q, 0.0];
            let value_storage = vec![
                None,
                Some(ValueData::scalar(cutoff)),
                Some(ValueData::scalar(q)),
                None,
            ];
            let value_refs: Vec<Option<&ValueData>> =
                value_storage.iter().map(|opt| opt.as_ref()).collect();
            let event_inputs: Vec<&[EventInstance]> = vec![&[]; scalars.len()];
            let mut pending = Vec::<PendingEvent>::new();
            let mut context =
                ProcessingContext::new(&scalars, &value_refs, &event_inputs, &mut pending);
            filter.input = input;
            filter.f_mod = 0.0;
            filter.process(sample_rate, &mut context);
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
