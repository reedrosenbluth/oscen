//! Looping mono sample playback with a runtime-swappable buffer.

use crate::asset::{AssetConsumer, AssetEndpoint, AssetError, AssetSlot, AudioAsset};
use crate::graph::SignalProcessor;
use crate::handoff;
use oscen_macros::Node;
use std::sync::Arc;

#[cfg(test)]
mod tests;

/// Off-thread builder: mixes an asset down to a single mono playback buffer.
/// Zero-sized; the build is just `to_mono` (may allocate, runs off the audio
/// thread inside the load handle's publish path).
#[derive(Debug, Default)]
pub struct SamplePlayerConsumer;

impl AssetConsumer for SamplePlayerConsumer {
    type Playable = Vec<f32>;
    fn build(&self, asset: &AudioAsset) -> Result<Vec<f32>, AssetError> {
        Ok(asset.to_mono())
    }
}

/// Looping mono sample-playback node with a runtime-swappable buffer.
///
/// The buffer arrives through the `#[input(asset)]` slot: a graph (or a test)
/// publishes a freshly decoded mono `Vec<f32>`, and `process` picks it up on
/// the next sample via a lock-free `take`, resets the playhead to 0, and hands
/// the old buffer back for off-thread destruction (`retire`). The swap path is
/// allocation-free. Playback loops; an unloaded player emits silence.
///
/// A swap resets the playhead to 0, which can click; this node favors
/// simplicity (the click-free crossfade pattern lives in `Convolver`).
#[derive(Debug, Node)]
pub struct SamplePlayer {
    #[output(stream)]
    pub output: f32,
    #[input(asset)]
    pub buf: AssetSlot<Vec<f32>>,
    current: Arc<Vec<f32>>,
    playhead: usize,
}

impl SamplePlayer {
    /// Empty player: emits silence until a buffer is published.
    pub fn new() -> Self {
        Self {
            output: 0.0,
            buf: AssetSlot::new(),
            current: Arc::new(Vec::new()),
            playhead: 0,
        }
    }

    /// Install the audio-side handoff consumer (the graph macro calls this via
    /// `AssetEndpoint::install_asset`; tests call it directly).
    pub fn install_buf_consumer(&mut self, consumer: handoff::Consumer<Vec<f32>>) {
        self.buf.install(consumer);
    }
}

impl Default for SamplePlayer {
    fn default() -> Self {
        Self::new()
    }
}

impl AssetEndpoint for SamplePlayer {
    type Consumer = SamplePlayerConsumer;
    fn install_asset(&mut self, consumer: handoff::Consumer<Vec<f32>>) {
        self.install_buf_consumer(consumer);
    }
}

impl SignalProcessor for SamplePlayer {
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
            self.output = 0.0;
            return;
        }

        self.output = self.current[self.playhead];
        self.playhead += 1;
        if self.playhead >= self.current.len() {
            self.playhead = 0;
        }
    }
}
