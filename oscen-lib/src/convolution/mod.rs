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

use crate::graph::{SampleRate, SignalProcessor};
use crate::spectral::{BlockAccumulator, Complex, FftPlan};
use oscen_macros::Node;

/// Taps covered by the zero-latency time-domain head (and block size of the
/// short FFT stage).
pub const SHORT_BLOCK_SIZE: usize = 32;
/// Block size of the long FFT stage; the short stage covers taps up to here.
pub const LONG_BLOCK_SIZE: usize = 512;

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

        self.fft
            .inverse(&mut self.spectrum_accum, &mut self.time_buf);
        self.output.copy_from_slice(&self.time_buf[b..]);
    }
}

/// Zero-latency impulse-response convolver node.
///
/// Three-tier Gardner decomposition: a time-domain head convolves taps
/// `[0, 32)` with zero latency, a short FFT stage (block 32, latency 32)
/// covers taps `[32, 512)`, and a long FFT stage (block 512, latency 512)
/// covers the rest. Because each stage's latency equals its segment's
/// offset, the summed output is the sample-exact full convolution with no
/// overall delay.
///
/// The impulse response is taken to be at the session sample rate; no
/// resampling is performed. Mono only.
#[derive(Debug, Node)]
pub struct Convolver {
    #[input(stream)]
    pub input: f32,
    #[output(stream)]
    pub output: f32,

    ir: Vec<f32>,
    head: DirectConvolver,
    short_stage: PartitionedConvolver,
    long_stage: PartitionedConvolver,
    sample_rate: SampleRate,
}

impl Convolver {
    /// Create a convolver for the given impulse response (assumed mono, at
    /// the session sample rate). An empty IR produces silence.
    pub fn new(ir: Vec<f32>) -> Self {
        let mut convolver = Self {
            input: 0.0,
            output: 0.0,
            ir,
            head: DirectConvolver::new(&[]),
            short_stage: PartitionedConvolver::new(SHORT_BLOCK_SIZE, &[]),
            long_stage: PartitionedConvolver::new(LONG_BLOCK_SIZE, &[]),
            sample_rate: SampleRate::default(),
        };
        convolver.rebuild_stages();
        convolver
    }

    /// Load a mono impulse response from a WAV file (multi-channel files
    /// are averaged down to mono; integer samples are normalized to ±1).
    pub fn from_wav(path: impl AsRef<std::path::Path>) -> Result<Self, hound::Error> {
        let mut reader = hound::WavReader::open(path)?;
        let spec = reader.spec();
        let channels = spec.channels.max(1) as usize;

        let mono: Vec<f32> = match spec.sample_format {
            hound::SampleFormat::Float => {
                let samples: Result<Vec<f32>, _> = reader.samples::<f32>().collect();
                mix_to_mono(&samples?, channels)
            }
            hound::SampleFormat::Int => {
                let scale = 1.0 / (1i64 << (spec.bits_per_sample - 1)) as f32;
                let samples: Result<Vec<i32>, _> = reader.samples::<i32>().collect();
                let scaled: Vec<f32> = samples?.iter().map(|&s| s as f32 * scale).collect();
                mix_to_mono(&scaled, channels)
            }
        };

        Ok(Self::new(mono))
    }

    /// Split the IR across the three tiers. The invariant: each stage's
    /// latency equals its segment's offset into the IR.
    fn rebuild_stages(&mut self) {
        let len = self.ir.len();

        let head_end = len.min(SHORT_BLOCK_SIZE);
        self.head = DirectConvolver::new(&self.ir[..head_end]);

        let short_end = len.min(LONG_BLOCK_SIZE);
        let short_segment = if len > SHORT_BLOCK_SIZE {
            &self.ir[SHORT_BLOCK_SIZE..short_end]
        } else {
            &[]
        };
        self.short_stage = PartitionedConvolver::new(SHORT_BLOCK_SIZE, short_segment);

        let long_segment = if len > LONG_BLOCK_SIZE {
            &self.ir[LONG_BLOCK_SIZE..]
        } else {
            &[]
        };
        self.long_stage = PartitionedConvolver::new(LONG_BLOCK_SIZE, long_segment);
    }
}

fn mix_to_mono(interleaved: &[f32], channels: usize) -> Vec<f32> {
    if channels == 1 {
        return interleaved.to_vec();
    }
    interleaved
        .chunks(channels)
        .map(|frame| frame.iter().sum::<f32>() / channels as f32)
        .collect()
}

impl SignalProcessor for Convolver {
    fn prepare(&mut self) {
        // The IR itself is rate-independent (taken to be at session rate),
        // but rebuilding returns every stage to a cleared, ready state.
        self.rebuild_stages();
    }

    #[inline]
    fn process(&mut self) {
        let x = self.input;
        self.output = self.head.process_sample(x)
            + self.short_stage.process_sample(x)
            + self.long_stage.process_sample(x);
    }
}
