//! Shared FFT utilities for spectral processing.
//!
//! This module is the allocation-aware kernel that spectral consumers build
//! on: [`FftPlan`] wraps a forward/inverse real FFT pair of a fixed size with
//! pre-allocated scratch (so the audio thread never allocates), and
//! [`BlockAccumulator`] adapts Oscen's per-sample `process()` model to
//! block-based algorithms by collecting samples until a block is full.
//!
//! The first consumer is [`crate::convolution`]. A windowed STFT helper
//! (window functions, overlap-add synthesis) is the intended extension point
//! for future consumers like a spectrum analyzer or spectral effects; it is
//! deliberately not implemented until one exists, since convolution uses
//! plain zero-padded blocks without windowing.

use std::sync::Arc;

use realfft::{ComplexToReal, RealFftPlanner, RealToComplex};

pub use realfft::num_complex::Complex;

/// A forward/inverse real-FFT pair of a fixed size with pre-allocated
/// scratch buffers.
///
/// All allocation happens in [`FftPlan::new`] (call it from a node's
/// `prepare()`); `forward` and `inverse` are allocation-free.
///
/// `inverse` is normalized by `1/size`, so a `forward` → `inverse`
/// round-trip reproduces the input.
pub struct FftPlan {
    size: usize,
    forward: Arc<dyn RealToComplex<f32>>,
    inverse: Arc<dyn ComplexToReal<f32>>,
    forward_scratch: Vec<Complex<f32>>,
    inverse_scratch: Vec<Complex<f32>>,
}

impl std::fmt::Debug for FftPlan {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FftPlan").field("size", &self.size).finish()
    }
}

impl FftPlan {
    /// Create a plan for real FFTs of `size` points. `size` must be even and
    /// non-zero (powers of two are fastest).
    pub fn new(size: usize) -> Self {
        assert!(
            size > 0 && size.is_multiple_of(2),
            "FFT size must be even and non-zero"
        );
        let mut planner = RealFftPlanner::<f32>::new();
        let forward = planner.plan_fft_forward(size);
        let inverse = planner.plan_fft_inverse(size);
        let forward_scratch = vec![Complex::default(); forward.get_scratch_len()];
        let inverse_scratch = vec![Complex::default(); inverse.get_scratch_len()];
        Self {
            size,
            forward,
            inverse,
            forward_scratch,
            inverse_scratch,
        }
    }

    /// The real FFT length this plan was built for.
    pub fn size(&self) -> usize {
        self.size
    }

    /// Number of complex bins produced by `forward`: `size / 2 + 1`.
    pub fn num_bins(&self) -> usize {
        self.size / 2 + 1
    }

    /// Allocate a zeroed spectrum buffer of the right length for this plan.
    pub fn make_spectrum(&self) -> Vec<Complex<f32>> {
        vec![Complex::default(); self.num_bins()]
    }

    /// Forward real FFT. `input.len()` must equal `size()` and
    /// `spectrum.len()` must equal `num_bins()`.
    ///
    /// `input` is used as working storage and its contents are not preserved.
    pub fn forward(&mut self, input: &mut [f32], spectrum: &mut [Complex<f32>]) {
        self.forward
            .process_with_scratch(input, spectrum, &mut self.forward_scratch)
            .expect("buffer lengths match the plan size");
    }

    /// Inverse real FFT, normalized by `1/size`. `spectrum.len()` must equal
    /// `num_bins()` and `output.len()` must equal `size()`.
    ///
    /// `spectrum` is used as working storage and its contents are not
    /// preserved.
    pub fn inverse(&mut self, spectrum: &mut [Complex<f32>], output: &mut [f32]) {
        // realfft requires the (mathematically always-zero) imaginary parts
        // of the DC and Nyquist bins to be exactly zero; clear any numerical
        // residue from spectral arithmetic.
        spectrum[0].im = 0.0;
        // The size is always even (asserted in `new`), so the last bin is
        // the Nyquist bin.
        spectrum[self.num_bins() - 1].im = 0.0;
        self.inverse
            .process_with_scratch(spectrum, output, &mut self.inverse_scratch)
            .expect("buffer lengths match the plan size");
        let scale = 1.0 / self.size as f32;
        for sample in output.iter_mut() {
            *sample *= scale;
        }
    }
}

/// Collects one sample per call until a block of `size` samples is full.
///
/// The adapter between Oscen's per-sample `process()` and block-based
/// spectral algorithms: call [`push`](Self::push) once per frame; when it
/// returns `true`, consume [`block`](Self::block) and call
/// [`clear`](Self::clear) to start the next block.
#[derive(Debug)]
pub struct BlockAccumulator {
    buffer: Vec<f32>,
    fill: usize,
}

impl BlockAccumulator {
    /// Create an accumulator for blocks of `size` samples. `size` must be
    /// non-zero.
    pub fn new(size: usize) -> Self {
        assert!(size > 0, "block size must be non-zero");
        Self {
            buffer: vec![0.0; size],
            fill: 0,
        }
    }

    /// Append one sample. Returns `true` when the block just became full.
    /// Pushing into a full block panics in debug builds; call
    /// [`clear`](Self::clear) first.
    pub fn push(&mut self, sample: f32) -> bool {
        debug_assert!(self.fill < self.buffer.len(), "push into full block");
        self.buffer[self.fill] = sample;
        self.fill += 1;
        self.fill == self.buffer.len()
    }

    /// The samples accumulated so far (the full block once `push` has
    /// returned `true`).
    pub fn block(&self) -> &[f32] {
        &self.buffer[..self.fill]
    }

    /// Discard the current block and start the next one.
    pub fn clear(&mut self) {
        self.fill = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fft_round_trip_is_identity() {
        let mut plan = FftPlan::new(64);
        let original: Vec<f32> = (0..64).map(|i| ((i * 7 + 3) % 13) as f32 - 6.0).collect();

        let mut time = original.clone();
        let mut spectrum = plan.make_spectrum();
        plan.forward(&mut time, &mut spectrum);

        let mut output = vec![0.0f32; 64];
        plan.inverse(&mut spectrum, &mut output);

        for (got, want) in output.iter().zip(original.iter()) {
            assert!(
                (got - want).abs() < 1e-4,
                "round trip mismatch: got {got}, want {want}"
            );
        }
    }

    #[test]
    fn fft_sizes_and_bins() {
        let plan = FftPlan::new(128);
        assert_eq!(plan.size(), 128);
        assert_eq!(plan.num_bins(), 65);
        assert_eq!(plan.make_spectrum().len(), 65);
    }

    #[test]
    fn forward_of_impulse_is_flat_spectrum() {
        let mut plan = FftPlan::new(32);
        let mut time = vec![0.0f32; 32];
        time[0] = 1.0;
        let mut spectrum = plan.make_spectrum();
        plan.forward(&mut time, &mut spectrum);

        for bin in &spectrum {
            assert!((bin.re - 1.0).abs() < 1e-5, "re = {}", bin.re);
            assert!(bin.im.abs() < 1e-5, "im = {}", bin.im);
        }
    }

    #[test]
    fn accumulator_reports_full_at_block_boundary() {
        let mut acc = BlockAccumulator::new(4);
        assert!(!acc.push(1.0));
        assert!(!acc.push(2.0));
        assert!(!acc.push(3.0));
        assert!(acc.push(4.0));
        assert_eq!(acc.block(), &[1.0, 2.0, 3.0, 4.0]);

        acc.clear();
        assert!(!acc.push(5.0));
        assert_eq!(acc.block(), &[5.0]);
        assert!(!acc.push(6.0));
        assert!(!acc.push(7.0));
        assert!(acc.push(8.0));
        assert_eq!(acc.block(), &[5.0, 6.0, 7.0, 8.0]);
    }
}
