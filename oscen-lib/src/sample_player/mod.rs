//! Looping sample playback with a runtime-swappable buffer, generic over the
//! frame type `F` (mono `f32` by default, `Frame<N>` for multi-channel).

use crate::asset::{AssetConsumer, AssetEndpoint, AssetError, AssetSlot, AudioAsset};
use crate::frame::AudioFrame;
use crate::graph::SignalProcessor;
use crate::handoff;
use oscen_macros::Node;
use std::marker::PhantomData;
use std::sync::Arc;

#[cfg(test)]
mod tests;

/// Off-thread builder: deinterleaves a channel-major asset into a `Vec<F>`
/// playback buffer, one `F` per source frame. Zero-sized (carries only the
/// frame type); the build may allocate and runs off the audio thread inside
/// the load handle's publish path.
#[derive(Debug)]
pub struct SamplePlayerConsumer<F: AudioFrame>(PhantomData<F>);

impl<F: AudioFrame> Default for SamplePlayerConsumer<F> {
    fn default() -> Self {
        Self(PhantomData)
    }
}

impl<F: AudioFrame> AssetConsumer for SamplePlayerConsumer<F> {
    type Playable = Vec<F>;

    /// Map the channel-major asset onto `F::CHANNELS`:
    /// - `src_ch == 1` (mono file) → broadcast channel 0 to every channel of `F`.
    /// - `src_ch >= F::CHANNELS` → take the first `F::CHANNELS` channels in order
    ///   (extra source channels dropped). For `F = f32` this is "channel 0 (left)
    ///   only", NOT a downmix average.
    /// - `1 < src_ch < F::CHANNELS` → map what exists; clamp the remaining target
    ///   channels to the last available source channel (`c.min(src_ch - 1)`).
    fn build(&self, asset: &AudioAsset) -> Result<Vec<F>, AssetError> {
        let frames = asset.frames();
        let src_ch = asset.channels();
        let out = (0..frames)
            .map(|t| {
                F::from_channels(|c| {
                    let sc = if src_ch == 1 { 0 } else { c.min(src_ch - 1) };
                    asset.channel(sc)[t]
                })
            })
            .collect();
        Ok(out)
    }
}

/// Looping sample-playback node with a runtime-swappable buffer, generic over
/// the frame type `F` (`f32` mono by default; `Frame<N>` for multi-channel).
///
/// The buffer arrives through the `#[input(asset)]` slot: a graph (or a test)
/// publishes a freshly decoded `Vec<F>`, and `process` picks it up on the next
/// sample via a lock-free `take`, resets the playhead to 0, and hands the old
/// buffer back for off-thread destruction (`retire`). The swap path is
/// allocation-free. Playback loops; an unloaded player emits silence.
///
/// A swap resets the playhead to 0, which can click; this node favors
/// simplicity (the click-free crossfade pattern lives in `Convolver`).
#[derive(Debug, Node)]
pub struct SamplePlayer<F: AudioFrame = f32> {
    #[output(stream)]
    pub output: F,
    #[input(asset)]
    pub buf: AssetSlot<Vec<F>>,
    current: Arc<Vec<F>>,
    playhead: usize,
}

impl<F: AudioFrame> SamplePlayer<F> {
    /// Empty player: emits silence until a buffer is published.
    pub fn new() -> Self {
        Self {
            output: F::default(),
            buf: AssetSlot::new(),
            current: Arc::new(Vec::new()),
            playhead: 0,
        }
    }

    /// Install the audio-side handoff consumer (the graph macro calls this via
    /// `AssetEndpoint::install_asset`; tests call it directly).
    pub fn install_buf_consumer(&mut self, consumer: handoff::Consumer<Vec<F>>) {
        self.buf.install(consumer);
    }
}

impl<F: AudioFrame> Default for SamplePlayer<F> {
    fn default() -> Self {
        Self::new()
    }
}

impl<F: AudioFrame> AssetEndpoint for SamplePlayer<F> {
    type Consumer = SamplePlayerConsumer<F>;
    fn install_asset(&mut self, consumer: handoff::Consumer<Vec<F>>) {
        self.install_buf_consumer(consumer);
    }
}

impl<F: AudioFrame> SignalProcessor for SamplePlayer<F> {
    #[inline]
    fn process(&mut self) {
        // Pick up a newly published buffer (RT-safe atomic swap). Reset the
        // playhead and hand the old buffer back for off-thread free — dropping
        // the old `Arc<Vec<f32>>` here would free on the audio thread.
        if let Some(new_buf) = self.buf.take() {
            let old = std::mem::replace(&mut self.current, new_buf);
            self.buf.retire(old);
            self.playhead = 0;
        }

        if self.current.is_empty() {
            self.output = F::default();
            return;
        }

        self.output = self.current[self.playhead];
        self.playhead += 1;
        if self.playhead >= self.current.len() {
            self.playhead = 0;
        }
    }
}
