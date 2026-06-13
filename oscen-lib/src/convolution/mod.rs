//! Impulse-response convolution.
//!
//! Building blocks for zero-latency partitioned convolution in the style of
//! Gardner ("Efficient Convolution without Input-Output Delay"):
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

/// Phase offset for the long stage's block schedule. Because
/// `LONG_BLOCK_SIZE` is a multiple of `SHORT_BLOCK_SIZE`, an un-shifted long
/// stage would run its FFT work on the same samples as the short stage every
/// long block. Any offset that is not a multiple of `SHORT_BLOCK_SIZE`
/// prevents that; half a short block keeps the two schedules maximally
/// apart.
const LONG_STAGE_PHASE_OFFSET: usize = SHORT_BLOCK_SIZE / 2;

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
    phase_offset: usize,
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
        Self::with_phase_offset(block_size, ir_segment, 0)
    }

    /// Like [`new`](Self::new), but with the block schedule shifted
    /// `phase_offset` samples earlier (`phase_offset < block_size`), as if
    /// the convolver had already consumed that many zeros. The output and
    /// the latency are unchanged — only *when* the per-block FFT work runs
    /// moves. Stages with different block sizes can use offsets to avoid
    /// running their FFT work on the same sample.
    pub fn with_phase_offset(block_size: usize, ir_segment: &[f32], phase_offset: usize) -> Self {
        assert!(block_size > 0, "block size must be non-zero");
        assert!(
            phase_offset < block_size,
            "phase offset must be less than the block size"
        );
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

        // Prime the accumulator: the convolver behaves as if `phase_offset`
        // zeros preceded the input, which moves block boundaries earlier by
        // that many samples without changing the output or the latency.
        let mut accumulator = BlockAccumulator::new(block_size);
        for _ in 0..phase_offset {
            accumulator.push(0.0);
        }

        Self {
            block_size,
            phase_offset,
            ir_spectra,
            input_spectra,
            fdl_pos: 0,
            accumulator,
            prev_block: vec![0.0; block_size],
            time_buf,
            spectrum_accum,
            output: vec![0.0; block_size],
            samples_processed: 0,
            fft,
        }
    }

    /// How many more samples until `process_sample` runs this convolver's
    /// per-block FFT work (1 means the next call does).
    pub fn samples_until_next_block(&self) -> usize {
        self.block_size - self.accumulator.block().len()
    }

    /// Process one input sample. The returned signal is the convolution of
    /// the input with the IR segment, delayed by exactly
    /// [`latency_samples`](Self::latency_samples).
    pub fn process_sample(&mut self, input: f32) -> f32 {
        if self.ir_spectra.is_empty() {
            return 0.0;
        }

        // Read before pushing: output sample t is y[t - B], so a block's
        // outputs are read back over the B samples after it is computed.
        // The phase offset shifts where y[t - B] sits inside its block
        // (blocks cover input ranges starting `phase_offset` early), not
        // when it is read.
        let t = self.samples_processed;
        let block = self.block_size as u64;
        let out = if t >= block {
            self.output[((t + self.phase_offset as u64) % block) as usize]
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
///
/// # CPU profile
///
/// The average cost per sample is low, but the work is bursty: each FFT
/// stage does all of its work for a block on the single `process()` call
/// where that block fills (a 64-point FFT round-trip every 32 samples for
/// the short stage; a 1024-point round-trip plus one spectral
/// multiply-accumulate per IR partition every 512 samples for the long
/// stage). The two stages' schedules are deliberately offset so their
/// bursts never land on the same sample. With typical audio callbacks
/// (≥ 64 samples per `process_block`) the bursts amortize inside the
/// callback budget; only very long IRs combined with very small callback
/// buffers can make the long stage's burst significant relative to the
/// budget. If that combination ever matters, the long stage's partition
/// multiplies can be amortized across the block at the cost of one extra
/// block of long-stage coverage — not currently implemented.
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
        self.long_stage = PartitionedConvolver::with_phase_offset(
            LONG_BLOCK_SIZE,
            long_segment,
            LONG_STAGE_PHASE_OFFSET,
        );
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
