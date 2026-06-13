//! Offline (non-realtime) block rendering helpers.
//!
//! `process_block` only accepts up to `DEFAULT_MAX_BLOCK_SIZE` frames at a
//! time because a graph's stream buffers are fixed-size stack arrays sized for
//! a realtime audio callback. `BlockRender` drives that bounded interface in a
//! loop so offline callers can render an arbitrary-length buffer in one call.
//!
//! Output is identical to realtime processing — this is a driver, not a
//! different algorithm. Value inputs keep whatever values were set before
//! `render` is called; only stream inputs are fed from the supplied buffers.

use crate::graph::DEFAULT_MAX_BLOCK_SIZE;

pub trait BlockRender {
    /// Number of stream inputs, in declaration order.
    const NUM_STREAM_INPUTS: usize;
    /// Number of stream outputs, in declaration order.
    const NUM_STREAM_OUTPUTS: usize;

    /// Advance the graph by `frames` (`<= DEFAULT_MAX_BLOCK_SIZE`). Forwards to
    /// the graph's inherent `process_block`; named distinctly to avoid clashing
    /// with it.
    fn run_block(&mut self, frames: usize);

    /// Mutable access to stream input `index`'s block buffer (length
    /// `DEFAULT_MAX_BLOCK_SIZE`), in declaration order.
    fn stream_input_block_mut(&mut self, index: usize) -> &mut [f32];

    /// Read access to stream output `index`'s block buffer, in declaration order.
    fn stream_output_block(&self, index: usize) -> &[f32];

    /// Render `inputs` (one slice per stream input, declaration order) followed
    /// by `tail` trailing zero frames, returning one buffer per stream output.
    ///
    /// Total length rendered = max input length + `tail`. Inputs shorter than
    /// that are zero-padded. `tail` lets effects with a decay (reverb, delay)
    /// ring out past the end of the input.
    ///
    /// # Panics
    /// If `inputs.len() != NUM_STREAM_INPUTS`.
    fn render(&mut self, inputs: &[&[f32]], tail: usize) -> Vec<Vec<f32>> {
        assert_eq!(
            inputs.len(),
            Self::NUM_STREAM_INPUTS,
            "render: expected {} input streams, got {}",
            Self::NUM_STREAM_INPUTS,
            inputs.len()
        );

        let in_len = inputs.iter().map(|s| s.len()).max().unwrap_or(0);
        let total = in_len + tail;

        let mut outs: Vec<Vec<f32>> = (0..Self::NUM_STREAM_OUTPUTS)
            .map(|_| Vec::with_capacity(total))
            .collect();

        if total == 0 || (Self::NUM_STREAM_INPUTS == 0 && Self::NUM_STREAM_OUTPUTS == 0) {
            return outs;
        }

        let cap = DEFAULT_MAX_BLOCK_SIZE;
        let mut pos = 0;
        while pos < total {
            let n = cap.min(total - pos);

            for i in 0..Self::NUM_STREAM_INPUTS {
                let src = inputs[i];
                let block = self.stream_input_block_mut(i);
                for j in 0..n {
                    block[j] = src.get(pos + j).copied().unwrap_or(0.0);
                }
            }

            self.run_block(n);

            for i in 0..Self::NUM_STREAM_OUTPUTS {
                let block = self.stream_output_block(i);
                outs[i].extend_from_slice(&block[..n]);
            }

            pos += n;
        }

        outs
    }

    /// Convenience wrapper for the common single-input, single-output graph.
    ///
    /// # Panics
    /// If the graph does not have exactly one stream input and one stream output.
    fn render_mono(&mut self, input: &[f32], tail: usize) -> Vec<f32> {
        assert_eq!(
            Self::NUM_STREAM_INPUTS,
            1,
            "render_mono: graph has {} stream inputs, expected 1",
            Self::NUM_STREAM_INPUTS
        );
        assert_eq!(
            Self::NUM_STREAM_OUTPUTS,
            1,
            "render_mono: graph has {} stream outputs, expected 1",
            Self::NUM_STREAM_OUTPUTS
        );
        self.render(&[input], tail)
            .pop()
            .expect("one output stream")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Mock 1-in/1-out graph: output = input * gain, one frame at a time, with
    /// block buffers sized exactly like a generated graph.
    struct MockGain {
        gain: f32,
        in_block: [f32; DEFAULT_MAX_BLOCK_SIZE],
        out_block: [f32; DEFAULT_MAX_BLOCK_SIZE],
    }

    impl MockGain {
        fn new(gain: f32) -> Self {
            Self {
                gain,
                in_block: [0.0; DEFAULT_MAX_BLOCK_SIZE],
                out_block: [0.0; DEFAULT_MAX_BLOCK_SIZE],
            }
        }
    }

    impl BlockRender for MockGain {
        const NUM_STREAM_INPUTS: usize = 1;
        const NUM_STREAM_OUTPUTS: usize = 1;

        fn run_block(&mut self, frames: usize) {
            for i in 0..frames {
                self.out_block[i] = self.in_block[i] * self.gain;
            }
        }

        fn stream_input_block_mut(&mut self, index: usize) -> &mut [f32] {
            match index {
                0 => &mut self.in_block,
                _ => panic!("stream input index {index} out of range"),
            }
        }

        fn stream_output_block(&self, index: usize) -> &[f32] {
            match index {
                0 => &self.out_block,
                _ => panic!("stream output index {index} out of range"),
            }
        }
    }

    /// Mock 0-in/1-out generator: emits a constant each frame.
    struct MockConst {
        value: f32,
        out_block: [f32; DEFAULT_MAX_BLOCK_SIZE],
    }

    impl BlockRender for MockConst {
        const NUM_STREAM_INPUTS: usize = 0;
        const NUM_STREAM_OUTPUTS: usize = 1;

        fn run_block(&mut self, frames: usize) {
            for i in 0..frames {
                self.out_block[i] = self.value;
            }
        }

        fn stream_input_block_mut(&mut self, index: usize) -> &mut [f32] {
            panic!("stream input index {index} out of range")
        }

        fn stream_output_block(&self, index: usize) -> &[f32] {
            match index {
                0 => &self.out_block,
                _ => panic!("stream output index {index} out of range"),
            }
        }
    }

    #[test]
    fn render_mono_applies_processing_within_one_block() {
        let mut g = MockGain::new(0.5);
        let input: Vec<f32> = (0..10).map(|i| i as f32).collect();
        let out = g.render_mono(&input, 0);
        assert_eq!(out.len(), 10);
        for (i, &v) in out.iter().enumerate() {
            assert_eq!(v, i as f32 * 0.5);
        }
    }

    #[test]
    fn render_spans_multiple_blocks() {
        let mut g = MockGain::new(2.0);
        let len = DEFAULT_MAX_BLOCK_SIZE * 2 + 37; // forces 3 chunks
        let input: Vec<f32> = (0..len).map(|i| (i % 7) as f32).collect();
        let out = g.render_mono(&input, 0);
        assert_eq!(out.len(), len);
        for (i, &v) in out.iter().enumerate() {
            assert_eq!(v, (i % 7) as f32 * 2.0, "mismatch at frame {i}");
        }
    }

    #[test]
    fn tail_appends_zero_input_frames() {
        let mut g = MockGain::new(1.0);
        let input = vec![1.0_f32; 5];
        let out = g.render_mono(&input, 8);
        assert_eq!(out.len(), 13);
        assert!(out[..5].iter().all(|&v| v == 1.0));
        assert!(out[5..].iter().all(|&v| v == 0.0), "tail should be silence");
    }

    #[test]
    fn generator_renders_tail_only() {
        let mut g = MockConst {
            value: 3.0,
            out_block: [0.0; DEFAULT_MAX_BLOCK_SIZE],
        };
        let out = g.render(&[], DEFAULT_MAX_BLOCK_SIZE + 5);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].len(), DEFAULT_MAX_BLOCK_SIZE + 5);
        assert!(out[0].iter().all(|&v| v == 3.0));
    }

    #[test]
    #[should_panic(expected = "expected 1 input streams")]
    fn render_rejects_wrong_input_count() {
        let mut g = MockGain::new(1.0);
        let _ = g.render(&[&[0.0], &[0.0]], 0);
    }
}
