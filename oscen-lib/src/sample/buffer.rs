//! Immutable, multi-channel sample data shared across the audio graph.
//!
//! A [`SampleBuffer`] is built entirely on a non-realtime (control) thread —
//! decoding, de-interleaving, any rate conversion — and then frozen behind an
//! `Arc`. Once constructed it is never mutated, so it can be shared across
//! threads and read from the audio thread with no synchronization beyond the
//! `Arc` itself. Readers (sample players, convolvers) hold an `Arc<SampleBuffer>`
//! and index into it with fractional positions.
//!
//! Storage is **planar**: all frames of channel 0, then all frames of channel
//! 1, and so on. Planar layout keeps a single channel's samples contiguous,
//! which is what an interpolating reader walks, so it is friendlier to the
//! cache than interleaved storage.

use crate::frame::Frame;

/// Immutable multi-channel PCM, `f32` per sample, planar layout.
#[derive(Clone, PartialEq)]
pub struct SampleBuffer {
    /// Planar samples: `[ch0 frame0..frameN, ch1 frame0..frameN, ...]`.
    data: Vec<f32>,
    channels: usize,
    frames: usize,
    /// Sample rate the data was recorded at. Players use this to play back at
    /// the correct pitch regardless of the graph's sample rate.
    source_rate: f32,
}

impl std::fmt::Debug for SampleBuffer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Don't dump the (potentially huge) sample vector.
        f.debug_struct("SampleBuffer")
            .field("channels", &self.channels)
            .field("frames", &self.frames)
            .field("source_rate", &self.source_rate)
            .finish()
    }
}

impl SampleBuffer {
    /// Build a buffer from already-planar data. `data.len()` must equal
    /// `channels * frames`.
    pub fn from_planar(data: Vec<f32>, channels: usize, source_rate: f32) -> Self {
        assert!(channels > 0, "SampleBuffer needs at least one channel");
        let frames = if channels == 0 { 0 } else { data.len() / channels };
        assert_eq!(
            data.len(),
            channels * frames,
            "planar data length must be a multiple of channel count"
        );
        Self {
            data,
            channels,
            frames,
            source_rate,
        }
    }

    /// Build a buffer from interleaved data (`[l0, r0, l1, r1, ...]`),
    /// de-interleaving into planar storage.
    pub fn from_interleaved(interleaved: &[f32], channels: usize, source_rate: f32) -> Self {
        assert!(channels > 0, "SampleBuffer needs at least one channel");
        let frames = interleaved.len() / channels;
        let mut data = vec![0.0; channels * frames];
        for frame in 0..frames {
            for ch in 0..channels {
                data[ch * frames + frame] = interleaved[frame * channels + ch];
            }
        }
        Self {
            data,
            channels,
            frames,
            source_rate,
        }
    }

    /// An all-silence buffer of the given size. Useful as a default before a
    /// real sample has been loaded.
    pub fn silent(frames: usize, channels: usize, source_rate: f32) -> Self {
        assert!(channels > 0, "SampleBuffer needs at least one channel");
        Self {
            data: vec![0.0; channels * frames],
            channels,
            frames,
            source_rate,
        }
    }

    #[inline]
    pub fn channels(&self) -> usize {
        self.channels
    }

    #[inline]
    pub fn frames(&self) -> usize {
        self.frames
    }

    #[inline]
    pub fn source_rate(&self) -> f32 {
        self.source_rate
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.frames == 0
    }

    /// Duration in seconds at the source sample rate.
    #[inline]
    pub fn duration_secs(&self) -> f32 {
        if self.source_rate > 0.0 {
            self.frames as f32 / self.source_rate
        } else {
            0.0
        }
    }

    /// Contiguous slice of one channel's samples (planar layout makes this free).
    #[inline]
    pub fn channel(&self, ch: usize) -> &[f32] {
        let start = ch * self.frames;
        &self.data[start..start + self.frames]
    }

    /// Raw sample at an integer `(channel, frame)`. Out-of-range frames read as
    /// zero so callers don't have to bounds-check the loop edges.
    #[inline]
    pub fn sample(&self, ch: usize, frame: usize) -> f32 {
        if ch >= self.channels || frame >= self.frames {
            0.0
        } else {
            self.data[ch * self.frames + frame]
        }
    }

    /// Linearly interpolated read of one channel at a fractional frame
    /// position. Positions outside `[0, frames - 1]` read as zero.
    #[inline]
    pub fn read_channel_linear(&self, ch: usize, pos: f64) -> f32 {
        if self.frames == 0 || pos < 0.0 {
            return 0.0;
        }
        let i = pos.floor();
        let idx = i as usize;
        if idx + 1 >= self.frames {
            // At or past the last frame: return the last sample (or 0 if past).
            return if idx < self.frames {
                self.sample(ch, idx)
            } else {
                0.0
            };
        }
        let frac = (pos - i) as f32;
        let base = ch * self.frames + idx;
        let a = self.data[base];
        let b = self.data[base + 1];
        a + (b - a) * frac
    }

    /// Linearly interpolated mono read: average of all channels at `pos`.
    #[inline]
    pub fn read_mono_linear(&self, pos: f64) -> f32 {
        if self.channels == 1 {
            return self.read_channel_linear(0, pos);
        }
        let mut sum = 0.0;
        for ch in 0..self.channels {
            sum += self.read_channel_linear(ch, pos);
        }
        sum / self.channels as f32
    }

    /// Linearly interpolated read into an `N`-channel frame. Buffer channels are
    /// mapped to frame channels positionally; if the buffer is mono it is
    /// broadcast to every frame channel, and missing channels read as zero.
    #[inline]
    pub fn read_frame_linear<const N: usize>(&self, pos: f64) -> Frame<N> {
        if self.channels == 1 {
            return Frame([self.read_channel_linear(0, pos); N]);
        }
        Frame(std::array::from_fn(|ch| {
            if ch < self.channels {
                self.read_channel_linear(ch, pos)
            } else {
                0.0
            }
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_interleaved_deinterleaves_to_planar() {
        // Two frames, stereo: frame0 = (1, 2), frame1 = (3, 4)
        let buf = SampleBuffer::from_interleaved(&[1.0, 2.0, 3.0, 4.0], 2, 44100.0);
        assert_eq!(buf.channels(), 2);
        assert_eq!(buf.frames(), 2);
        assert_eq!(buf.channel(0), &[1.0, 3.0]);
        assert_eq!(buf.channel(1), &[2.0, 4.0]);
    }

    #[test]
    fn linear_interpolation_midpoint() {
        let buf = SampleBuffer::from_planar(vec![0.0, 1.0], 1, 44100.0);
        assert_eq!(buf.read_channel_linear(0, 0.0), 0.0);
        assert_eq!(buf.read_channel_linear(0, 0.5), 0.5);
        assert_eq!(buf.read_channel_linear(0, 1.0), 1.0);
    }

    #[test]
    fn out_of_range_reads_zero() {
        let buf = SampleBuffer::from_planar(vec![0.5, 0.5], 1, 44100.0);
        assert_eq!(buf.read_channel_linear(0, -1.0), 0.0);
        assert_eq!(buf.read_channel_linear(0, 5.0), 0.0);
    }

    #[test]
    fn mono_read_averages_channels() {
        let buf = SampleBuffer::from_interleaved(&[1.0, 3.0], 2, 44100.0);
        assert_eq!(buf.read_mono_linear(0.0), 2.0);
    }

    #[test]
    fn read_frame_broadcasts_mono() {
        let buf = SampleBuffer::from_planar(vec![0.25], 1, 44100.0);
        let f: Frame<2> = buf.read_frame_linear(0.0);
        assert_eq!(f, Frame([0.25, 0.25]));
    }

    #[test]
    fn duration_matches_frames_over_rate() {
        let buf = SampleBuffer::silent(44100, 1, 44100.0);
        assert!((buf.duration_secs() - 1.0).abs() < 1e-6);
    }
}
