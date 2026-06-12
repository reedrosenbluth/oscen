//! Impulse-response convolution.
//!
//! Building blocks for zero-latency partitioned convolution in the style of
//! Gardner ("Efficient Convolution without Input-Output Delay") and Cmajor's
//! `std::convolution`:
//!
//! - [`DirectConvolver`]: brute-force time-domain convolution. Zero latency,
//!   O(taps) per sample — used for the short head of an impulse response.
//! - [`PartitionedConvolver`]: uniform-partition FFT convolution with a
//!   latency of exactly one block. Used for the body and tail of an impulse
//!   response, where its O(log n)-ish per-sample cost wins.
//!
//! The alignment invariant that makes a multi-stage convolver sum to a
//! sample-exact full convolution: **a stage's latency in samples must equal
//! the offset of its IR segment**. A `PartitionedConvolver` with block size
//! `B` has latency `B`, so it must be fed the IR samples starting at offset
//! `B` relative to whatever earlier stages cover.

use crate::spectral::{BlockAccumulator, Complex, FftPlan};

/// Brute-force time-domain FIR convolution with zero latency.
#[derive(Debug)]
pub struct DirectConvolver {
    taps: Vec<f32>,
    history: Vec<f32>,
    write_pos: usize,
}

impl DirectConvolver {
    /// Create a convolver for the given taps (`taps[0]` multiplies the
    /// current sample). An empty tap list produces silence.
    pub fn new(taps: &[f32]) -> Self {
        Self {
            taps: taps.to_vec(),
            history: vec![0.0; taps.len().max(1)],
            write_pos: 0,
        }
    }

    /// Process one input sample, returning the convolved output with zero
    /// latency.
    pub fn process_sample(&mut self, input: f32) -> f32 {
        if self.taps.is_empty() {
            return 0.0;
        }
        let len = self.history.len();
        self.history[self.write_pos] = input;

        let mut acc = 0.0f32;
        let mut idx = self.write_pos;
        for &tap in &self.taps {
            acc += tap * self.history[idx];
            idx = if idx == 0 { len - 1 } else { idx - 1 };
        }

        self.write_pos = (self.write_pos + 1) % len;
        acc
    }
}

/// Uniform-partition FFT convolution (frequency-domain delay line,
/// overlap-save) with a latency of exactly `block_size` samples.
#[derive(Debug)]
pub struct PartitionedConvolver {
    block_size: usize,
    fft: FftPlan,
    ir_spectra: Vec<Vec<Complex<f32>>>,
    input_spectra: Vec<Vec<Complex<f32>>>,
    fdl_pos: usize,
    accumulator: BlockAccumulator,
    prev_block: Vec<f32>,
    time_buf: Vec<f32>,
    spectrum_accum: Vec<Complex<f32>>,
    output: Vec<f32>,
    samples_processed: u64,
}

impl PartitionedConvolver {
    /// Create a convolver for `ir_segment`, processed in partitions of
    /// `block_size` samples (must be non-zero; powers of two are fastest).
    /// The segment is zero-padded to a whole number of partitions. An empty
    /// segment produces silence.
    pub fn new(block_size: usize, ir_segment: &[f32]) -> Self {
        assert!(block_size > 0, "block size must be non-zero");
        let mut fft = FftPlan::new(2 * block_size);
        let mut time_buf = vec![0.0f32; 2 * block_size];

        // Pre-transform each IR partition, zero-padded to the FFT size.
        let partitions = ir_segment.len().div_ceil(block_size);
        let mut ir_spectra = Vec::with_capacity(partitions);
        for chunk in ir_segment.chunks(block_size) {
            time_buf.fill(0.0);
            time_buf[..chunk.len()].copy_from_slice(chunk);
            let mut spectrum = fft.make_spectrum();
            fft.forward(&mut time_buf, &mut spectrum);
            ir_spectra.push(spectrum);
        }

        let input_spectra = (0..partitions).map(|_| fft.make_spectrum()).collect();
        let spectrum_accum = fft.make_spectrum();

        Self {
            block_size,
            ir_spectra,
            input_spectra,
            fdl_pos: 0,
            accumulator: BlockAccumulator::new(block_size),
            prev_block: vec![0.0; block_size],
            time_buf,
            spectrum_accum,
            output: vec![0.0; block_size],
            samples_processed: 0,
            fft,
        }
    }

    /// Process one input sample. The returned signal is the convolution of
    /// the input with the IR segment, delayed by exactly
    /// [`latency_samples`](Self::latency_samples).
    pub fn process_sample(&mut self, input: f32) -> f32 {
        if self.ir_spectra.is_empty() {
            return 0.0;
        }

        // Read before pushing: output sample t is y[t - B], so the block
        // computed when sample (k+1)B - 1 arrives is read back while samples
        // (k+1)B .. (k+2)B - 1 arrive.
        let t = self.samples_processed;
        let block = self.block_size as u64;
        let out = if t >= block {
            self.output[(t % block) as usize]
        } else {
            0.0
        };
        self.samples_processed += 1;

        if self.accumulator.push(input) {
            self.process_block();
            self.accumulator.clear();
        }

        out
    }

    /// The fixed processing delay: one block.
    pub fn latency_samples(&self) -> usize {
        self.block_size
    }

    /// Overlap-save step for one completed input block: FFT the sliding
    /// 2B-sample window, multiply-accumulate against every IR partition
    /// (each delayed by its position in the frequency-domain delay line),
    /// inverse FFT, and keep the alias-free second half.
    fn process_block(&mut self) {
        let b = self.block_size;
        let partitions = self.ir_spectra.len();

        self.time_buf[..b].copy_from_slice(&self.prev_block);
        self.time_buf[b..].copy_from_slice(self.accumulator.block());
        self.prev_block.copy_from_slice(self.accumulator.block());

        // The delay line ring grows backwards: the newest spectrum lives at
        // fdl_pos, the one from p blocks ago at (fdl_pos + p) % partitions.
        self.fdl_pos = (self.fdl_pos + partitions - 1) % partitions;
        self.fft
            .forward(&mut self.time_buf, &mut self.input_spectra[self.fdl_pos]);

        for bin in self.spectrum_accum.iter_mut() {
            *bin = Complex::default();
        }
        for (p, ir_spectrum) in self.ir_spectra.iter().enumerate() {
            let input_spectrum = &self.input_spectra[(self.fdl_pos + p) % partitions];
            for ((acc, &x), &h) in self
                .spectrum_accum
                .iter_mut()
                .zip(input_spectrum)
                .zip(ir_spectrum)
            {
                *acc += x * h;
            }
        }

        self.fft.inverse(&mut self.spectrum_accum, &mut self.time_buf);
        self.output.copy_from_slice(&self.time_buf[b..]);
    }
}
