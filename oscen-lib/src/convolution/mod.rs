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

use crate::asset::{AssetConsumer, AssetEndpoint, AssetError, AssetSlot, AudioAsset};
use crate::frame::AudioFrame;
use crate::graph::{SampleRate, SignalProcessor};
use crate::handoff;
use crate::spectral::{BlockAccumulator, Complex, FftPlan};
use arrayvec::ArrayVec;
use oscen_macros::Node;
use std::marker::PhantomData;
use std::sync::Arc;

#[cfg(test)]
mod tests;

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

/// The playable convolution state for one IR: the three Gardner stages plus
/// the IR they were built from. Built off the audio thread; the audio thread
/// only runs [`process_sample`](Self::process_sample) and (on `prepare`)
/// [`rebuild`](Self::rebuild).
#[derive(Debug)]
pub struct ConvolverEngine {
    ir: Vec<f32>,
    head: DirectConvolver,
    short_stage: PartitionedConvolver,
    long_stage: PartitionedConvolver,
}

impl ConvolverEngine {
    /// Split `ir` across the three tiers. An empty IR yields a silent engine.
    pub fn from_ir(ir: Vec<f32>) -> Self {
        let mut engine = Self {
            ir,
            head: DirectConvolver::new(&[]),
            short_stage: PartitionedConvolver::new(SHORT_BLOCK_SIZE, &[]),
            long_stage: PartitionedConvolver::new(LONG_BLOCK_SIZE, &[]),
        };
        engine.rebuild();
        engine
    }

    /// Re-split the stored IR, returning every stage to a cleared, ready state.
    /// Allocates (called from `prepare`, never the RT path). The invariant:
    /// each stage's latency equals its segment's offset into the IR.
    pub fn rebuild(&mut self) {
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

    /// Sum the three stages for one input sample.
    #[inline]
    pub fn process_sample(&mut self, x: f32) -> f32 {
        self.head.process_sample(x)
            + self.short_stage.process_sample(x)
            + self.long_stage.process_sample(x)
    }
}

/// Maximum channels a [`MultiConvolverEngine`] holds. Stereo needs 2; the cap
/// keeps the per-channel bundle stack-friendly via `ArrayVec`.
pub const MAX_CONV_CHANNELS: usize = 8;

/// One mono [`ConvolverEngine`] per channel: the realtime-playable state for a
/// multi-channel IR. Channel `c` of the input is convolved with channel `c` of
/// the IR (the standard stereo-reverb topology — L→L, R→R, no L↔R cross terms).
/// Built off the audio thread; the audio thread only runs
/// [`process_frame`](Self::process_frame) and (on `prepare`)
/// [`rebuild`](Self::rebuild).
#[derive(Debug)]
pub struct MultiConvolverEngine {
    channels: ArrayVec<ConvolverEngine, MAX_CONV_CHANNELS>,
}

impl MultiConvolverEngine {
    /// Build `num_channels` per-channel engines from a channel-major IR asset.
    /// The channel mapping mirrors the sample player's clamp rule:
    /// - mono IR (`asset.channels() == 1`) broadcasts channel 0 to every engine;
    /// - an IR with `>= num_channels` channels maps the first `num_channels`
    ///   channels in order (extra source channels dropped);
    /// - `1 < asset.channels() < num_channels` maps what exists and clamps the
    ///   remaining engines to the last available source channel.
    ///
    /// Off-thread; allocates (one FFT prep per channel).
    pub fn from_asset(asset: &AudioAsset, num_channels: usize) -> Self {
        debug_assert!(
            num_channels <= MAX_CONV_CHANNELS,
            "num_channels {num_channels} exceeds MAX_CONV_CHANNELS {MAX_CONV_CHANNELS}"
        );
        let src_ch = asset.channels();
        let mut channels = ArrayVec::new();
        for c in 0..num_channels {
            let sc = if src_ch == 1 { 0 } else { c.min(src_ch - 1) };
            channels.push(ConvolverEngine::from_ir(asset.channel(sc).to_vec()));
        }
        Self { channels }
    }

    /// Build `num_channels` engines that all share one mono IR (broadcast).
    /// Backs the mono `with_ir`/`new` helpers on a channel-generic node: a mono
    /// IR plays identically on every channel. Off-thread; allocates.
    pub fn from_mono_ir(ir: &[f32], num_channels: usize) -> Self {
        debug_assert!(
            num_channels <= MAX_CONV_CHANNELS,
            "num_channels {num_channels} exceeds MAX_CONV_CHANNELS {MAX_CONV_CHANNELS}"
        );
        let mut channels = ArrayVec::new();
        for _ in 0..num_channels {
            channels.push(ConvolverEngine::from_ir(ir.to_vec()));
        }
        Self { channels }
    }

    /// Number of per-channel engines.
    pub fn num_channels(&self) -> usize {
        self.channels.len()
    }

    /// Rebuild every per-channel engine, returning each stage to a cleared,
    /// ready state. Allocates; called from `prepare`, never the RT path.
    pub fn rebuild(&mut self) {
        for engine in &mut self.channels {
            engine.rebuild();
        }
    }

    /// Convolve one multi-channel frame: channel `c` of `x` through engine `c`.
    /// Requires `F::CHANNELS == self.num_channels()`. Allocation-free.
    #[inline]
    pub fn process_frame<F: AudioFrame>(&mut self, x: F) -> F {
        debug_assert_eq!(
            F::CHANNELS,
            self.channels.len(),
            "frame channels must match engine channels"
        );
        let channels = &mut self.channels;
        F::from_channels(|c| channels[c].process_sample(x.channel(c)))
    }
}

/// Builds a [`MultiConvolverEngine`] with `F::CHANNELS` per-channel engines from
/// an IR asset. Zero-sized (carries only the frame type); the build is the
/// off-thread per-channel FFT prep.
#[derive(Debug)]
pub struct ConvolverConsumer<F: AudioFrame = f32>(PhantomData<F>);

impl<F: AudioFrame> Default for ConvolverConsumer<F> {
    fn default() -> Self {
        Self(PhantomData)
    }
}

impl<F: AudioFrame> AssetConsumer for ConvolverConsumer<F> {
    type Playable = MultiConvolverEngine;
    fn build(&self, asset: &AudioAsset) -> Result<MultiConvolverEngine, AssetError> {
        Ok(MultiConvolverEngine::from_asset(asset, F::CHANNELS))
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
/// resampling is performed.
///
/// Generic over the frame type `F` (mono `f32` by default; `Frame<N>` for
/// multi-channel). A multi-channel convolver holds one mono engine per channel
/// and convolves channel `c` of the input with channel `c` of the IR — the
/// standard stereo-reverb topology (L→L, R→R), no L↔R cross terms.
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
///
/// The live IR can be swapped at runtime: a graph publishes a freshly-built
/// [`MultiConvolverEngine`] through an [`AssetSlot`], and `process` crossfades
/// from the outgoing engine to the new one over [`CROSSFADE_SECONDS`]
/// (equal-power, click-free, applied per channel). The swap path is
/// allocation-free — the new engine is `take`n from the handoff and the retired
/// one handed back for off-thread destruction.
#[derive(Debug, Node)]
pub struct Convolver<F: AudioFrame = f32> {
    #[input(stream)]
    pub input: F,
    #[output(stream)]
    pub output: F,

    current: Arc<MultiConvolverEngine>,
    fading: Option<(Arc<MultiConvolverEngine>, usize)>,
    fade_len: usize,
    #[input(asset)]
    pub ir: AssetSlot<MultiConvolverEngine>,
    sample_rate: SampleRate,
}

/// Crossfade duration for a live IR swap (20 ms; the spec allows 10–50 ms).
const CROSSFADE_SECONDS: f32 = 0.02;

impl<F: AudioFrame> Convolver<F> {
    /// Empty convolver: passes silence until an asset is published. Used by the
    /// graph (sub-project 4) where the IR arrives via the asset input.
    pub fn new() -> Self {
        Self {
            input: F::default(),
            output: F::default(),
            current: Arc::new(MultiConvolverEngine::from_mono_ir(&[], F::CHANNELS)),
            fading: None,
            fade_len: 1,
            ir: AssetSlot::new(),
            sample_rate: SampleRate::default(),
        }
    }

    /// Convolver with a mono IR baked in at construction, broadcast to every
    /// channel of `F`. No live swapping unless an asset consumer is later
    /// installed. An empty IR produces silence.
    ///
    /// The taps are assumed to be at the session sample rate; they are not
    /// resampled. To load an impulse response from a file at any rate, use the
    /// asset input (`#[input(asset)] ir`), which resamples the file to the
    /// session rate and preserves its channels.
    pub fn with_ir(ir: Vec<f32>) -> Self {
        Self {
            current: Arc::new(MultiConvolverEngine::from_mono_ir(&ir, F::CHANNELS)),
            ..Self::new()
        }
    }

    /// Install the audio-side handoff consumer (sub-project 4's macro calls
    /// this; tests call it directly). Delegates to the asset slot.
    pub fn install_ir_consumer(&mut self, consumer: handoff::Consumer<MultiConvolverEngine>) {
        self.ir.install(consumer);
    }
}

impl<F: AudioFrame> Default for Convolver<F> {
    fn default() -> Self {
        Self::new()
    }
}

impl<F: AudioFrame> AssetEndpoint for Convolver<F> {
    type Consumer = ConvolverConsumer<F>;

    fn install_asset(&mut self, consumer: handoff::Consumer<MultiConvolverEngine>) {
        self.install_ir_consumer(consumer);
    }
}

impl<F: AudioFrame> SignalProcessor for Convolver<F> {
    fn prepare(&mut self) {
        self.fade_len = ((CROSSFADE_SECONDS * self.sample_rate.0).round() as usize).max(1);
        // The IR is rate-independent (taken to be at session rate), but
        // rebuilding returns every stage to a cleared, ready state. The current
        // engine is uniquely owned at prepare (no swap has happened yet).
        Arc::get_mut(&mut self.current)
            .expect("engine uniquely owned at prepare")
            .rebuild();
        self.fading = None;
    }

    #[inline]
    fn process(&mut self) {
        // 1. Pull a newly published engine, if any (RT-safe: atomic swap).
        if let Some(new_engine) = self.ir.take() {
            if let Some((old, _)) = self.fading.take() {
                // A second swap landed mid-fade: retire the in-progress
                // outgoing engine now (keeps at most two engines live), fade
                // from the current engine.
                self.ir.retire(old);
            }
            let prev = std::mem::replace(&mut self.current, new_engine);
            self.fading = Some((prev, 0));
        }

        let x = self.input;
        let new_out = Arc::get_mut(&mut self.current)
            .expect("current engine uniquely owned")
            .process_frame::<F>(x);

        self.output = match self.fading.as_mut() {
            None => new_out,
            Some((old, pos)) => {
                let old_out = Arc::get_mut(old)
                    .expect("outgoing engine uniquely owned")
                    .process_frame::<F>(x);
                let g = (*pos as f32) / (self.fade_len as f32); // 0..=1
                                                                // Equal-power crossfade: sin²+cos² = 1, no level dip.
                                                                // Scalar gains broadcast to every channel via `Mul<f32>`.
                let gain_new = (g * std::f32::consts::FRAC_PI_2).sin();
                let gain_old = (g * std::f32::consts::FRAC_PI_2).cos();
                let mixed = new_out * gain_new + old_out * gain_old;
                *pos += 1;
                if *pos >= self.fade_len {
                    let (old_arc, _) = self.fading.take().unwrap();
                    self.ir.retire(old_arc); // hand back for off-thread free
                }
                mixed
            }
        };
    }
}
