//! Loading sample data from files. **Control-thread only** — decoding allocates
//! and does I/O; the audio thread only ever sees a finished `Arc<SampleBuffer>`.

use std::path::Path;

use super::buffer::SampleBuffer;

/// Errors that can occur while loading a sample file.
#[derive(Debug)]
pub enum LoadError {
    /// Underlying WAV decoder error.
    Wav(hound::Error),
    /// The file decoded to zero frames.
    Empty,
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LoadError::Wav(e) => write!(f, "wav decode error: {e}"),
            LoadError::Empty => write!(f, "sample file contained no audio"),
        }
    }
}

impl std::error::Error for LoadError {}

impl From<hound::Error> for LoadError {
    fn from(e: hound::Error) -> Self {
        LoadError::Wav(e)
    }
}

/// Load a WAV file into a [`SampleBuffer`], normalizing integer PCM to
/// `[-1.0, 1.0]` and preserving the file's channel count and sample rate.
pub fn load_wav<P: AsRef<Path>>(path: P) -> Result<SampleBuffer, LoadError> {
    let reader = hound::WavReader::open(path)?;
    decode(reader)
}

/// Decode a WAV from any reader (e.g. an in-memory cursor).
pub fn read_wav<R: std::io::Read>(reader: R) -> Result<SampleBuffer, LoadError> {
    let reader = hound::WavReader::new(reader)?;
    decode(reader)
}

fn decode<R: std::io::Read>(mut reader: hound::WavReader<R>) -> Result<SampleBuffer, LoadError> {
    let spec = reader.spec();
    let channels = spec.channels.max(1) as usize;
    let source_rate = spec.sample_rate as f32;

    let interleaved: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader
            .samples::<f32>()
            .collect::<Result<Vec<_>, _>>()?,
        hound::SampleFormat::Int => {
            // hound decodes any integer bit depth into i32; normalize by the
            // full-scale value for the declared bit depth.
            let scale = 1.0 / (1i64 << (spec.bits_per_sample - 1)) as f32;
            reader
                .samples::<i32>()
                .map(|s| s.map(|v| v as f32 * scale))
                .collect::<Result<Vec<_>, _>>()?
        }
    };

    if interleaved.is_empty() {
        return Err(LoadError::Empty);
    }

    Ok(SampleBuffer::from_interleaved(
        &interleaved,
        channels,
        source_rate,
    ))
}
