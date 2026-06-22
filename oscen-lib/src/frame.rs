//! Multi-channel audio frames: one sample-instant across N channels.

use std::iter::Sum;
use std::ops::{Add, Mul, Neg, Sub};

/// One sample-instant across N channels. The audio-standard "frame".
///
/// `f32` is the canonical mono type; `Frame<N>` is the multi-channel value a
/// stream edge can carry. Element type is fixed to `f32` by design — see the
/// design spec, decision #4.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Frame<const N: usize>(pub [f32; N]);

impl<const N: usize> Default for Frame<N> {
    #[inline]
    fn default() -> Self {
        Frame([0.0; N])
    }
}

/// Stereo frame: left/right. Reads lighter than `Frame<2>` in type positions.
pub type Stereo = Frame<2>;

/// Quad frame: four channels.
pub type Quad = Frame<4>;

/// Single-channel frame. NOTE: `f32` is the canonical mono type (design spec,
/// decision #2) — `Frame<1>` is a permitted degenerate case, not normalized to.
/// Reach for this alias only for genuinely `N == 1` frame code (e.g. generic
/// node code instantiated at one channel), never as a replacement for mono `f32`.
pub type Mono = Frame<1>;

/// Construct a frame from its channels: `[l, r].into()` instead of `Frame([l, r])`.
impl<const N: usize> From<[f32; N]> for Frame<N> {
    #[inline]
    fn from(samples: [f32; N]) -> Self {
        Frame(samples)
    }
}

/// Scalar broadcast: one sample fills every channel (Cmajor-style scalar→vector).
/// `let dc: Stereo = 0.5.into();` yields `Frame([0.5, 0.5])`.
impl<const N: usize> From<f32> for Frame<N> {
    #[inline]
    fn from(sample: f32) -> Self {
        Frame([sample; N])
    }
}

/// The value type a stream carries: `f32` (mono) or `Frame<N>` (multi-channel).
///
/// This is the single bound the resampler kernels and the arithmetic codegen
/// target. The arithmetic surface (`Add`/`Sub`/`Mul<f32>`/`Sum`) is exactly what
/// the resampler kernels need to be written once, generically, with no per-channel
/// loop: element-wise add/sub for accumulation and fan-in, scalar-broadcast
/// `frame * f32` for tap weights and gain.
pub trait AudioFrame:
    Copy
    + Default
    + Send
    + std::fmt::Debug
    + Add<Output = Self>
    + Sub<Output = Self>
    + Mul<f32, Output = Self>
    + Sum
{
    /// Number of channels in one frame.
    const CHANNELS: usize;

    /// Snap each channel to zero when its magnitude is below `threshold`.
    /// Guards the recursive all-pass state in the IIR halfband against the
    /// ~100× denormal-multiply slowdown on x86. Applied per channel for frames,
    /// so each channel flushes independently of the others.
    fn flush_denormal(self, threshold: f32) -> Self;

    /// Construct a frame by sampling each channel index in `0..CHANNELS`.
    fn from_channels(f: impl FnMut(usize) -> f32) -> Self;

    /// The sample in channel `index` (`index < CHANNELS`).
    fn channel(&self, index: usize) -> f32;
}

impl AudioFrame for f32 {
    const CHANNELS: usize = 1;
    #[inline]
    fn flush_denormal(self, threshold: f32) -> Self {
        if self.abs() < threshold {
            0.0
        } else {
            self
        }
    }
    #[inline]
    fn from_channels(mut f: impl FnMut(usize) -> f32) -> Self {
        f(0)
    }
    #[inline]
    fn channel(&self, _index: usize) -> f32 {
        *self
    }
}

impl<const N: usize> Add for Frame<N> {
    type Output = Self;
    #[inline]
    fn add(self, rhs: Self) -> Self {
        Frame(core::array::from_fn(|i| self.0[i] + rhs.0[i]))
    }
}

impl<const N: usize> Sub for Frame<N> {
    type Output = Self;
    #[inline]
    fn sub(self, rhs: Self) -> Self {
        Frame(core::array::from_fn(|i| self.0[i] - rhs.0[i]))
    }
}

impl<const N: usize> Mul<f32> for Frame<N> {
    type Output = Self;
    #[inline]
    fn mul(self, rhs: f32) -> Self {
        Frame(core::array::from_fn(|i| self.0[i] * rhs))
    }
}

impl<const N: usize> Neg for Frame<N> {
    type Output = Self;
    #[inline]
    fn neg(self) -> Self {
        Frame(core::array::from_fn(|i| -self.0[i]))
    }
}

impl<const N: usize> Sum for Frame<N> {
    #[inline]
    fn sum<I: Iterator<Item = Self>>(iter: I) -> Self {
        iter.fold(Frame::default(), |acc, f| acc + f)
    }
}

impl<const N: usize> AudioFrame for Frame<N> {
    const CHANNELS: usize = N;
    #[inline]
    fn flush_denormal(self, threshold: f32) -> Self {
        Frame(core::array::from_fn(|i| {
            if self.0[i].abs() < threshold {
                0.0
            } else {
                self.0[i]
            }
        }))
    }
    #[inline]
    fn from_channels(f: impl FnMut(usize) -> f32) -> Self {
        Frame(core::array::from_fn(f))
    }
    #[inline]
    fn channel(&self, index: usize) -> f32 {
        self.0[index]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_all_zeros() {
        assert_eq!(Frame::<2>::default(), Frame([0.0, 0.0]));
        assert_eq!(Frame::<4>::default(), Frame([0.0; 4]));
    }

    #[test]
    fn construct_and_index_channels() {
        let f = Frame([0.25_f32, -0.5]);
        assert_eq!(f.0[0], 0.25);
        assert_eq!(f.0[1], -0.5);
    }

    #[test]
    fn equality_is_elementwise() {
        assert_eq!(Frame([1.0, 2.0]), Frame([1.0, 2.0]));
        assert_ne!(Frame([1.0, 2.0]), Frame([1.0, 2.5]));
    }

    #[test]
    fn audioframe_channels_const() {
        assert_eq!(<f32 as AudioFrame>::CHANNELS, 1);
    }

    fn assert_is_audioframe<F: AudioFrame>() {}

    #[test]
    fn f32_is_audioframe() {
        assert_is_audioframe::<f32>();
    }

    #[test]
    fn add_is_elementwise() {
        assert_eq!(Frame([1.0, 2.0]) + Frame([0.5, -1.0]), Frame([1.5, 1.0]));
    }

    #[test]
    fn sub_is_elementwise() {
        assert_eq!(Frame([1.0, 2.0]) - Frame([0.5, -1.0]), Frame([0.5, 3.0]));
    }

    #[test]
    fn mul_f32_broadcasts() {
        assert_eq!(Frame([1.0, -2.0]) * 0.5, Frame([0.5, -1.0]));
    }

    #[test]
    fn neg_is_elementwise() {
        assert_eq!(-Frame([1.0, -2.0]), Frame([-1.0, 2.0]));
    }

    #[test]
    fn sum_folds_elementwise() {
        let frames = [Frame([1.0, 10.0]), Frame([2.0, 20.0]), Frame([3.0, 30.0])];
        let total: Frame<2> = frames.into_iter().sum();
        assert_eq!(total, Frame([6.0, 60.0]));
    }

    #[test]
    fn empty_sum_is_default() {
        let total: Frame<2> = std::iter::empty::<Frame<2>>().sum();
        assert_eq!(total, Frame::<2>::default());
    }

    #[test]
    fn frame_is_audioframe() {
        assert_is_audioframe::<Frame<2>>();
        assert_eq!(<Frame<2> as AudioFrame>::CHANNELS, 2);
        assert_eq!(<Frame<4> as AudioFrame>::CHANNELS, 4);
    }

    #[test]
    fn flush_denormal_f32_snaps_below_threshold() {
        assert_eq!(<f32 as AudioFrame>::flush_denormal(1e-20, 1e-15), 0.0);
        assert_eq!(<f32 as AudioFrame>::flush_denormal(-1e-20, 1e-15), 0.0);
        assert_eq!(<f32 as AudioFrame>::flush_denormal(0.5, 1e-15), 0.5);
    }

    #[test]
    fn from_array_constructs_channels() {
        let f: Frame<2> = [0.25, -0.5].into();
        assert_eq!(f, Frame([0.25, -0.5]));
    }

    #[test]
    fn from_scalar_broadcasts_to_all_channels() {
        let f: Frame<2> = 0.5.into();
        assert_eq!(f, Frame([0.5, 0.5]));
        let q: Quad = 1.0.into();
        assert_eq!(q, Frame([1.0; 4]));
    }

    #[test]
    fn aliases_are_transparent() {
        let s: Stereo = [0.1, 0.2].into();
        assert_eq!(<Stereo as AudioFrame>::CHANNELS, 2);
        assert_eq!(<Mono as AudioFrame>::CHANNELS, 1);
        assert_eq!(s, Frame([0.1, 0.2]));
    }

    #[test]
    fn flush_denormal_frame_is_per_channel() {
        // One sub-threshold channel snaps; the other is preserved untouched.
        let f = Frame([1e-20_f32, 0.5]).flush_denormal(1e-15);
        assert_eq!(f, Frame([0.0, 0.5]));
    }

    #[test]
    fn from_channels_builds_frame_per_index() {
        let f = Frame::<2>::from_channels(|i| (i + 1) as f32 * 10.0);
        assert_eq!(f, Frame([10.0, 20.0]));
    }

    #[test]
    fn from_channels_f32_takes_channel_zero() {
        assert_eq!(f32::from_channels(|_| 0.7), 0.7);
    }

    #[test]
    fn channel_reads_indexed_sample() {
        assert_eq!(Frame([1.0, 2.0]).channel(1), 2.0);
    }

    #[test]
    fn channel_f32_ignores_index() {
        assert_eq!((0.7f32).channel(0), 0.7);
    }

    #[test]
    fn from_channels_channel_round_trip() {
        let frame = Frame([0.25_f32, -0.5]);
        let rebuilt = Frame::<2>::from_channels(|i| frame.channel(i));
        assert_eq!(rebuilt, frame);
    }
}
