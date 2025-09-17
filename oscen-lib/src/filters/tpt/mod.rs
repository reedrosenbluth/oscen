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
    d: f32,
    a: f32,
    g1: f32,

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
        let freq = self.cutoff.clamp(20.0, sample_rate * 0.48);
        let period = 0.5 / sample_rate;
        let f = (2.0 * sample_rate) * (2.0 * PI * freq * period).tan() * period;
        let inv_q = 1.0 / self.q;

        self.d = 1.0 / (1.0 + inv_q * f + f * f);
        self.a = f;
        self.g1 = f + inv_q;
    }
}

impl SignalProcessor for TptFilter {
    fn init(&mut self, sample_rate: f32) {
        self.update_coefficients(sample_rate);
    }

    fn process(&mut self, sample_rate: f32, inputs: &[f32]) -> f32 {
        let input = self.get_input(inputs);

        if self.frame_counter == 0 {
            let cutoff = self.get_cutoff(inputs).clamp(20.0, sample_rate * 0.5);
            let q = self.get_q(inputs).clamp(0.1, 10.0);

            if cutoff != self.cutoff || q != self.q {
                self.cutoff = cutoff;
                self.q = q;
                self.update_coefficients(sample_rate);
            }
        }

        self.frame_counter = (self.frame_counter + 1) % self.frames_per_update;

        let high = (input - self.g1 * self.z[0] - self.z[1]) * self.d;
        let band = self.a * high + self.z[0];
        let low = self.a * band + self.z[1];

        self.z[0] = self.a * high + band;
        self.z[1] = self.a * band + low;

        self.output = low;
        self.output
    }
}
