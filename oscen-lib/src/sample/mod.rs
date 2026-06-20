//! Audio sample "externals": load arbitrary samples, share them across the
//! graph, and swap them in realtime.
//!
//! The pieces fit together in three layers:
//!
//! 1. [`SampleBuffer`] — immutable, multi-channel PCM, built off the audio
//!    thread (decode it with [`load_wav`], or construct it directly).
//! 2. [`SampleSlot`] / [`SampleBank`] — a realtime-swappable handle to one
//!    buffer, and a named registry of such handles (the "`buffer~`" identity
//!    layer). The control thread swaps data; audio-thread readers pick up the
//!    change without blocking or allocating.
//! 3. Reader nodes such as [`SamplePlayer`] — DSP nodes that read a slot.
//!
//! The same slot/bank machinery is generic over its payload, so a future
//! convolution node can share a prepared impulse-response kernel through the
//! exact same realtime-safe swap path.
//!
//! ## Quick start
//!
//! ```ignore
//! use oscen::sample;
//!
//! // Control thread: load a file and publish it under a name.
//! let kick = sample::load_wav("kick.wav").unwrap();
//! sample::global_bank().store("kick", std::sync::Arc::new(kick));
//!
//! // In a graph: reference the buffer by name (a string literal works inside
//! // the `graph!` macro because it captures nothing from the surrounding scope).
//! // nodes { player = SamplePlayer::from_buffer("kick"); }
//!
//! // Later, swap the sample with no glitch:
//! let snare = sample::load_wav("snare.wav").unwrap();
//! sample::global_bank().store("kick", std::sync::Arc::new(snare));
//! ```

mod buffer;
mod io;
mod player;
mod slot;

pub use buffer::SampleBuffer;
pub use io::{load_wav, read_wav, LoadError};
pub use player::SamplePlayer;
pub use slot::{SampleBank, SampleSlot};

use std::sync::{Arc, OnceLock};

/// The process-global named sample registry used by
/// [`SamplePlayer::from_buffer`]. Reference buffers by name in graphs and swap
/// their data here from the control thread.
pub fn global_bank() -> &'static SampleBank {
    static BANK: OnceLock<SampleBank> = OnceLock::new();
    BANK.get_or_init(SampleBank::new)
}

/// Convenience: load a WAV file and publish it into the global bank under
/// `name`, returning the loaded buffer's handle. **Control thread only.**
pub fn load_into_bank<P: AsRef<std::path::Path>>(
    name: &str,
    path: P,
) -> Result<Arc<SampleBuffer>, LoadError> {
    let buffer = Arc::new(load_wav(path)?);
    global_bank().store(name, Arc::clone(&buffer));
    Ok(buffer)
}
