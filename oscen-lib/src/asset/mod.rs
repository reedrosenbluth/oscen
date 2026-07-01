//! Immutable audio buffers (`AudioAsset`) and their off-thread loaders.
//!
//! An [`AudioAsset`] is a deinterleaved (channel-major), immutable buffer at a
//! known sample rate, cheap to clone (samples sit behind an `Arc`). It is built
//! only off the audio thread via [`AudioAsset::from_wav`] /
//! [`AudioAsset::from_samples`]; every decode/validation failure surfaces as an
//! [`AssetError`] on the calling (non-realtime) thread.

use crate::handoff::{self, Consumer, Publisher};
use std::path::Path;
use std::sync::Arc;

mod resample;

/// An immutable, deinterleaved (channel-major) audio buffer at a known sample
/// rate. Cheap to clone (the samples sit behind an `Arc`). Constructed only
/// off the audio thread via the loaders below.
#[derive(Clone, Debug)]
pub struct AudioAsset {
    samples: Arc<[f32]>, // channel-major: all of ch0, then all of ch1, ...
    channels: usize,
    frames: usize,
    sample_rate: u32,
}

/// Failure modes of asset loading. All are returned on the calling (non-RT)
/// thread; the audio thread never sees a `Result` from this subsystem.
#[derive(Debug)]
pub enum AssetError {
    /// WAV open/decode/format failure from `hound`.
    Decode(hound::Error),
    /// The decoded buffer had zero frames.
    Empty,
    /// The asset declared a sample rate of 0 (e.g. a malformed WAV header),
    /// so it cannot be resampled to the graph rate.
    ZeroSampleRate,
    /// The graph rate is not yet configured, so the asset cannot be conformed
    /// to it. Loads with a valid graph rate are resampled on the fly, so a
    /// rate difference alone is never a load error — this only fires when a
    /// load is attempted before `set_graph_rate`/`init`.
    GraphRateUnset { asset: u32 },
    /// A pre-built asset was published into a handle whose graph rate differs
    /// from the asset's. Assets are conformed to the graph rate when they are
    /// built (`from_samples`/`from_wav`); rebuild the asset at `graph` Hz.
    SampleRateMismatch { asset: u32, graph: u32 },
}

impl std::fmt::Display for AssetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AssetError::Decode(err) => write!(f, "{err}"),
            AssetError::Empty => write!(f, "audio asset is empty"),
            AssetError::ZeroSampleRate => {
                write!(f, "audio asset declares a sample rate of 0 Hz")
            }
            AssetError::GraphRateUnset { asset } => write!(
                f,
                "cannot conform asset ({asset} Hz): graph rate not configured \
                 (call set_graph_rate/init first)"
            ),
            AssetError::SampleRateMismatch { asset, graph } => write!(
                f,
                "asset is at {asset} Hz but the graph runs at {graph} Hz; \
                 rebuild the asset at the graph rate"
            ),
        }
    }
}

impl std::error::Error for AssetError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            AssetError::Decode(err) => Some(err),
            _ => None,
        }
    }
}

impl From<hound::Error> for AssetError {
    fn from(err: hound::Error) -> Self {
        AssetError::Decode(err)
    }
}

impl AudioAsset {
    /// Number of channels (≥ 1).
    pub fn channels(&self) -> usize {
        self.channels
    }

    /// Number of frames (> 0 — the loaders reject empty buffers).
    pub fn frames(&self) -> usize {
        self.frames
    }

    /// The asset's sample rate in Hz.
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Channel `index` as a contiguous slice (length == frames). Panics if
    /// `index >= channels`. This contiguity is the reason for channel-major
    /// storage — convolution borrows one channel directly.
    pub fn channel(&self, index: usize) -> &[f32] {
        assert!(
            index < self.channels,
            "channel index {index} out of range (channels = {})",
            self.channels
        );
        &self.samples[index * self.frames..(index + 1) * self.frames]
    }

    /// Average all channels to a single mono buffer (length == frames).
    /// A 1-channel asset returns a copy of its only channel.
    pub fn to_mono(&self) -> Vec<f32> {
        if self.channels == 1 {
            return self.channel(0).to_vec();
        }
        let inv = 1.0 / self.channels as f32;
        let mut mono = vec![0.0; self.frames];
        for ch in 0..self.channels {
            let slice = self.channel(ch);
            for (m, &s) in mono.iter_mut().zip(slice.iter()) {
                *m += s;
            }
        }
        for m in mono.iter_mut() {
            *m *= inv;
        }
        mono
    }

    /// Decode a WAV file. Integer formats normalize to ±1.0 f32; the result is
    /// deinterleaved to channel-major. `graph_rate` is the graph's sample rate;
    /// a file at a different rate is resampled to it (see [`from_samples`]).
    ///
    /// [`from_samples`]: AudioAsset::from_samples
    pub fn from_wav(path: impl AsRef<Path>, graph_rate: u32) -> Result<AudioAsset, AssetError> {
        let mut reader = hound::WavReader::open(path)?;
        let spec = reader.spec();
        let channels = spec.channels.max(1) as usize;

        let interleaved: Vec<f32> = match spec.sample_format {
            hound::SampleFormat::Float => reader.samples::<f32>().collect::<Result<_, _>>()?,
            hound::SampleFormat::Int => {
                let scale = 1.0 / (1i64 << (spec.bits_per_sample - 1)) as f32;
                reader
                    .samples::<i32>()
                    .map(|s| s.map(|v| v as f32 * scale))
                    .collect::<Result<_, _>>()?
            }
        };

        Self::from_samples(interleaved, channels, spec.sample_rate, graph_rate)
    }

    /// Build an asset from in-memory **interleaved** (frame-major) samples —
    /// used by tests and by future in-memory consumers. `samples.len()` must be
    /// a multiple of `channels`, `channels >= 1`, and the buffer non-empty.
    ///
    /// If `rate != graph_rate` the samples are resampled per channel to
    /// `graph_rate` (band-limited; no aliasing) so the stored asset is always
    /// at the graph rate. A `rate` of 0 is a [`ZeroSampleRate`] error; a
    /// `graph_rate` of 0 means the rate has not been configured yet and is a
    /// [`GraphRateUnset`] error.
    ///
    /// [`ZeroSampleRate`]: AssetError::ZeroSampleRate
    /// [`GraphRateUnset`]: AssetError::GraphRateUnset
    pub fn from_samples(
        samples: Vec<f32>,
        channels: usize,
        rate: u32,
        graph_rate: u32,
    ) -> Result<AudioAsset, AssetError> {
        if samples.is_empty() {
            return Err(AssetError::Empty);
        }
        // `channels == 0` or a ragged buffer is a programming error; there is no
        // dedicated variant (the spec keeps `AssetError` minimal), so `Empty` is
        // the closest fit.
        if channels == 0 || !samples.len().is_multiple_of(channels) {
            return Err(AssetError::Empty);
        }
        let src_frames = samples.len() / channels;
        if src_frames == 0 {
            return Err(AssetError::Empty);
        }
        // A zero asset rate (malformed WAV header) cannot be resampled;
        // reject it here so `resample_channel` never sees a zero rate.
        if rate == 0 {
            return Err(AssetError::ZeroSampleRate);
        }
        // A rate difference is resolved by resampling, but only once the graph
        // rate is known; loading before `set_graph_rate`/`init` cannot conform.
        if graph_rate == 0 {
            return Err(AssetError::GraphRateUnset { asset: rate });
        }

        // Deinterleave frame-major → channel-major.
        let mut out = vec![0.0f32; samples.len()];
        for (frame_index, frame) in samples.chunks(channels).enumerate() {
            for (c, &sample) in frame.iter().enumerate() {
                out[c * src_frames + frame_index] = sample;
            }
        }

        // Conform to the graph rate. Resampling is per channel on the
        // channel-major layout; an unchanged rate is a cheap copy.
        let (samples, frames) = if rate == graph_rate {
            (out, src_frames)
        } else {
            let mut resampled: Vec<f32> = Vec::new();
            let mut dst_frames = 0;
            for c in 0..channels {
                let channel = &out[c * src_frames..(c + 1) * src_frames];
                let conformed = resample::resample_channel(channel, rate, graph_rate);
                dst_frames = conformed.len();
                resampled.extend_from_slice(&conformed);
            }
            (resampled, dst_frames)
        };
        if frames == 0 {
            return Err(AssetError::Empty);
        }

        Ok(AudioAsset {
            samples: samples.into(),
            channels,
            frames,
            sample_rate: graph_rate,
        })
    }
}

/// Turns an [`AudioAsset`] into a node's playable state. `build` runs OFF the
/// audio thread (inside the load handle's publish path), so it may allocate and
/// do heavy DSP prep (e.g. FFT-partitioning an impulse response).
pub trait AssetConsumer {
    /// The realtime-playable state built from an asset (must cross the handoff).
    type Playable: Send;

    /// Build the playable state from `asset`. Off-thread; may allocate.
    fn build(&self, asset: &AudioAsset) -> Result<Self::Playable, AssetError>;
}

/// Non-realtime load handle: decode + build + publish. A graph exposes one of
/// these per `external` asset input (sub-project 4); usable standalone here.
/// All work happens on the calling (control) thread, never the audio thread.
pub struct AssetLoadHandle<C: AssetConsumer> {
    publisher: Publisher<C::Playable>,
    builder: C,
    graph_rate: u32,
}

impl<C: AssetConsumer> AssetLoadHandle<C> {
    /// Wrap a handoff publisher and a consumer/builder. The graph rate starts
    /// at 0 (unset); call [`set_graph_rate`](Self::set_graph_rate) before
    /// loading from WAV.
    pub fn new(publisher: Publisher<C::Playable>, builder: C) -> Self {
        Self {
            publisher,
            builder,
            graph_rate: 0,
        }
    }

    /// Record the graph's sample rate, used to validate WAV loads. Called at
    /// graph init.
    pub fn set_graph_rate(&mut self, rate: u32) {
        self.graph_rate = rate;
    }

    /// Build from an already-loaded asset and publish it to the audio thread.
    /// If the graph rate is configured, the asset must already be at it —
    /// assets are conformed when built, so a mismatch here means the asset was
    /// built against a different rate.
    pub fn publish(&mut self, asset: &AudioAsset) -> Result<(), AssetError> {
        if self.graph_rate != 0 && asset.sample_rate() != self.graph_rate {
            return Err(AssetError::SampleRateMismatch {
                asset: asset.sample_rate(),
                graph: self.graph_rate,
            });
        }
        let playable = self.builder.build(asset)?;
        self.publisher.publish(playable);
        Ok(())
    }

    /// Decode a WAV at the stored graph rate, build, and publish it.
    pub fn load_wav(&mut self, path: impl AsRef<Path>) -> Result<(), AssetError> {
        let asset = AudioAsset::from_wav(path, self.graph_rate)?;
        self.publish(&asset)
    }
}

impl<C: AssetConsumer> std::fmt::Debug for AssetLoadHandle<C> {
    // The handoff publisher is not `Debug`; print just the type name so a graph
    // embedding an `AssetLoadHandle` field can still derive `Debug`.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("AssetLoadHandle")
    }
}

/// Implemented by a node that has exactly one asset input, so the `graph!`
/// macro can wire a handoff to it generically. The macro learns, from only the
/// node's *type*: (a) the [`AssetConsumer`] (hence the `Playable` that crosses
/// the handoff), (b) how to install the audio-side consumer, and (c) the
/// off-thread builder. Everything else in the generated wiring is generic.
pub trait AssetEndpoint {
    /// The node's asset consumer (off-thread builder of the playable state).
    type Consumer: AssetConsumer + Default;

    /// Install the audio-side handoff consumer into the node's asset slot.
    fn install_asset(&mut self, consumer: Consumer<<Self::Consumer as AssetConsumer>::Playable>);

    /// The off-thread builder paired with the load handle.
    fn asset_builder() -> Self::Consumer {
        Self::Consumer::default()
    }
}

/// A node's asset input: the audio-side handoff consumer (absent until a graph
/// installs one). [`take`](Self::take)/[`retire`](Self::retire) are RT-safe and
/// total — they are no-ops when no consumer is installed.
pub struct AssetSlot<T: Send> {
    consumer: Option<Consumer<T>>,
}

impl<T: Send> AssetSlot<T> {
    /// An empty slot (no consumer installed yet).
    pub fn new() -> Self {
        Self { consumer: None }
    }

    /// Install the audio-side handoff consumer.
    pub fn install(&mut self, consumer: Consumer<T>) {
        self.consumer = Some(consumer);
    }

    /// Pull a newly published value, if any. `None` if nothing is published or
    /// no consumer is installed. RT-safe.
    pub fn take(&mut self) -> Option<Arc<T>> {
        self.consumer.as_mut().and_then(handoff::Consumer::take)
    }

    /// Hand a retired value back for off-thread destruction. A no-op if no
    /// consumer is installed. RT-safe.
    pub fn retire(&mut self, value: Arc<T>) {
        if let Some(consumer) = self.consumer.as_mut() {
            consumer.retire(value);
        }
    }
}

impl<T: Send> Default for AssetSlot<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Send> std::fmt::Debug for AssetSlot<T> {
    // The inner handoff types are not `Debug`; print just the slot name so
    // nodes embedding an `AssetSlot` can still derive `Debug`.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("AssetSlot")
    }
}

#[cfg(test)]
mod tests;
