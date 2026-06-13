//! Offline convolution-reverb renderer for testing the `Convolver` node.
//!
//! Loads an input WAV and an impulse-response WAV, runs every sample of the
//! input through a `Convolver` (one graph instance per channel so stereo is
//! preserved), pads the tail so the reverb rings out, peak-normalizes to avoid
//! clipping, and writes a WAV you can listen to.
//!
//! Usage:
//!   cargo run -p oscen-examples --bin render_convolution -- <input.wav> <ir.wav> [output.wav]
//!
//! Default output path is `<input>_reverb.wav` next to the input file.

use hound::{SampleFormat, WavReader, WavSpec, WavWriter};
use oscen::prelude::*;
use std::path::PathBuf;
use std::sync::OnceLock;

/// The IR is loaded at runtime, but the `graph!` macro constructs nodes with
/// no arguments. Stash the loaded IR here so the node constructor can read it.
static IR: OnceLock<Vec<f32>> = OnceLock::new();

fn impulse_response() -> Vec<f32> {
    IR.get()
        .expect("IR not set before graph construction")
        .clone()
}

graph! {
    name: ReverbRenderGraph;

    input stream dry;
    output stream wet;

    nodes {
        reverb = Convolver::new(impulse_response());
    }

    connections {
        dry -> reverb.input;
        reverb.output -> wet;
    }
}

/// Read a WAV into deinterleaved per-channel f32 buffers (±1 range).
fn read_wav(path: &str) -> Result<(Vec<Vec<f32>>, u32), Box<dyn std::error::Error>> {
    let mut reader = WavReader::open(path)?;
    let spec = reader.spec();
    let channels = spec.channels.max(1) as usize;

    let interleaved: Vec<f32> = match spec.sample_format {
        SampleFormat::Float => reader.samples::<f32>().collect::<Result<_, _>>()?,
        SampleFormat::Int => {
            let scale = 1.0 / (1i64 << (spec.bits_per_sample - 1)) as f32;
            reader
                .samples::<i32>()
                .map(|s| s.map(|v| v as f32 * scale))
                .collect::<Result<_, _>>()?
        }
    };

    let frames = interleaved.len() / channels;
    let mut deinterleaved = vec![Vec::with_capacity(frames); channels];
    for frame in interleaved.chunks(channels) {
        for (ch, &sample) in frame.iter().enumerate() {
            deinterleaved[ch].push(sample);
        }
    }
    Ok((deinterleaved, spec.sample_rate))
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!(
            "usage: {} <input.wav> <ir.wav> [output.wav]",
            args.first()
                .map(String::as_str)
                .unwrap_or("render_convolution")
        );
        std::process::exit(2);
    }
    let input_path = &args[1];
    let ir_path = &args[2];
    let output_path = args.get(3).cloned().unwrap_or_else(|| {
        let mut p = PathBuf::from(input_path);
        let stem = p
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("output")
            .to_string();
        p.set_file_name(format!("{stem}_reverb.wav"));
        p.to_string_lossy().into_owned()
    });

    // Load input and IR.
    let (input_channels, input_rate) = read_wav(input_path)?;
    let (ir_channels, ir_rate) = read_wav(ir_path)?;

    if ir_rate != input_rate {
        eprintln!(
            "warning: IR sample rate ({ir_rate} Hz) differs from input ({input_rate} Hz). \
             The Convolver does not resample, so the reverb time will be scaled by {:.3}x.",
            input_rate as f32 / ir_rate as f32
        );
    }

    // Mix the IR down to mono (matching Convolver::from_wav's behavior).
    let ir_frames = ir_channels.first().map(Vec::len).unwrap_or(0);
    let ir_ch_count = ir_channels.len().max(1);
    let ir: Vec<f32> = (0..ir_frames)
        .map(|f| ir_channels.iter().map(|c| c[f]).sum::<f32>() / ir_ch_count as f32)
        .collect();
    let tail = ir.len();
    IR.set(ir).expect("IR already set");

    println!(
        "input: {} ch, {} frames @ {} Hz | IR: {} taps ({:.2}s) | tail pad: {} frames",
        input_channels.len(),
        input_channels.first().map(Vec::len).unwrap_or(0),
        input_rate,
        tail,
        tail as f32 / input_rate as f32,
        tail
    );

    // Render each channel through its own graph (same mono IR per channel).
    let mut wet_channels: Vec<Vec<f32>> = Vec::with_capacity(input_channels.len());
    for dry in &input_channels {
        let mut graph = ReverbRenderGraph::new();
        graph.init(input_rate as f32);
        // `tail` extra zero frames let the reverb ring out past the input.
        wet_channels.push(graph.render_mono(dry, tail));
    }

    // Peak-normalize across all channels to just under full scale.
    let peak = wet_channels
        .iter()
        .flat_map(|c| c.iter())
        .fold(0.0f32, |m, &s| m.max(s.abs()));
    let gain = if peak > 0.999 { 0.999 / peak } else { 1.0 };
    if gain != 1.0 {
        println!("peak {peak:.3} -> normalizing by {gain:.4}");
    } else {
        println!("peak {peak:.3} (no normalization needed)");
    }

    // Interleave and write a float WAV.
    let channels = wet_channels.len();
    let frames = wet_channels.iter().map(Vec::len).max().unwrap_or(0);
    let spec = WavSpec {
        channels: channels as u16,
        sample_rate: input_rate,
        bits_per_sample: 32,
        sample_format: SampleFormat::Float,
    };
    let mut writer = WavWriter::create(&output_path, spec)?;
    for f in 0..frames {
        for ch in &wet_channels {
            writer.write_sample(ch.get(f).copied().unwrap_or(0.0) * gain)?;
        }
    }
    writer.finalize()?;

    println!("wrote {output_path}");
    Ok(())
}
