//! Exact integer sample delay: the node behind the `graph!` inline-delay
//! bracket `src -> [N] -> dst`.

use crate::graph::{AllowsFeedback, SignalProcessor};
use oscen_macros::Node;

/// Delays its input by a fixed whole number of samples, exactly:
/// `output[t] = input[t - delay]`, so a delay of 0 is a passthrough.
///
/// `graph!` expands `src -> [N] -> dst` to `src -> SampleDelay::new(N - 1)`
/// followed by a feedback edge into `dst`. A feedback edge reads its source
/// as it was at the start of the frame, which adds the remaining sample, so
/// the bracket delays by exactly `N` wherever the nodes are scheduled.
///
/// The buffer is allocated in [`SampleDelay::new`]; `process` never
/// allocates.
#[derive(Debug, Node)]
pub struct SampleDelay {
    #[input(stream)]
    pub input: f32,

    #[output(stream)]
    pub output: f32,

    buffer: Box<[f32]>,
    pos: usize,
}

impl SampleDelay {
    /// A delay of `delay` samples.
    pub fn new(delay: usize) -> Self {
        Self {
            input: 0.0,
            output: 0.0,
            buffer: vec![0.0; delay].into_boxed_slice(),
            pos: 0,
        }
    }

    /// The delay in samples.
    pub fn delay(&self) -> usize {
        self.buffer.len()
    }
}

impl SignalProcessor for SampleDelay {
    #[inline(always)]
    fn process(&mut self) {
        if self.buffer.is_empty() {
            self.output = self.input;
            return;
        }
        // `buffer[pos]` was written `len` calls ago.
        self.output = self.buffer[self.pos];
        self.buffer[self.pos] = self.input;
        self.pos += 1;
        if self.pos == self.buffer.len() {
            self.pos = 0;
        }
    }
}

impl AllowsFeedback for SampleDelay {}

#[cfg(test)]
mod tests {
    use super::*;

    fn impulse_response(delay: usize, len: usize) -> Vec<f32> {
        let mut d = SampleDelay::new(delay);
        (0..len)
            .map(|t| {
                d.input = if t == 0 { 1.0 } else { 0.0 };
                d.process();
                d.output
            })
            .collect()
    }

    #[test]
    fn delays_by_exactly_the_commanded_samples() {
        for delay in 0..6 {
            let out = impulse_response(delay, 12);
            for (t, &y) in out.iter().enumerate() {
                assert_eq!(
                    y,
                    if t == delay { 1.0 } else { 0.0 },
                    "delay {delay}, t {t}"
                );
            }
        }
    }

    #[test]
    fn passes_a_signal_through_unchanged_across_wrap_around() {
        let mut d = SampleDelay::new(3);
        let input: Vec<f32> = (0..50).map(|t| t as f32).collect();
        for (t, &x) in input.iter().enumerate() {
            d.input = x;
            d.process();
            let want = if t >= 3 { input[t - 3] } else { 0.0 };
            assert_eq!(d.output, want);
        }
    }
}
