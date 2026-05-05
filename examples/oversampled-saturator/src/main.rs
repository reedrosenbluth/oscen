#![feature(inherent_associated_types)]
//! Oversampled hard-clip saturator integration example.
//!
//! Drives a 2 kHz saw oscillator through a hard-clip non-linearity. At 1×
//! (no oversampling) the clipper produces audible alias components; at 4×
//! the saw is upsampled before clipping, the non-linearity runs at 4× the
//! base rate, and a sinc anti-imaging filter brings the signal back down,
//! pushing image energy above the audible band.
//!
//! ## Run
//!
//! ```text
//! OSCEN_FACTOR=1 cargo run -p oversampled-saturator
//! OSCEN_FACTOR=4 cargo run -p oversampled-saturator
//! ```
//!
//! When no audio device is available (CI, headless), the program falls back
//! to a compute-only smoke test that prints the first few output samples.

#![allow(non_camel_case_types)]

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use oscen::{oversample_variants, Node, PolyBlepOscillator, SignalProcessor};
use std::thread;
use std::time::Duration;

/// Memoryless hard-clip non-linearity. Clipping a band-limited signal
/// generates harmonics of arbitrarily high order — the textbook source of
/// aliasing in digital audio. Oversampling moves the alias components above
/// the audible range before downsampling filters them away.
#[derive(Debug, Node)]
pub struct HardClip {
    #[input(stream)]
    pub input: f32,

    #[output(stream)]
    pub output: f32,
}

impl HardClip {
    pub fn new() -> Self {
        Self {
            input: 0.0,
            output: 0.0,
        }
    }
}

impl Default for HardClip {
    fn default() -> Self {
        Self::new()
    }
}

impl SignalProcessor for HardClip {
    fn process(&mut self) {
        // Drive then clip. A high-amplitude saw guarantees the clipper is
        // engaged for nearly the entire cycle, maximizing harmonic content.
        let driven = self.input * 1.5;
        self.output = driven.clamp(-0.7, 0.7);
    }
}

oversample_variants! {
    base_name: SatGraph;
    factors: [1, 4];
    body: {
        output stream audio_out;

        nodes {
            osc = PolyBlepOscillator::saw(2_000.0, 0.6) * {FACTOR};
            clip = HardClip::new() * {FACTOR};
        }

        connections {
            osc.output -> clip.input;
            [sinc] clip.output -> audio_out;
        }
    }
}

fn main() {
    let factor: u32 = std::env::var("OSCEN_FACTOR")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1);

    println!("oversampled-saturator: factor = {factor}x");

    match factor {
        1 => run_1x(),
        4 => run_4x(),
        other => panic!("OSCEN_FACTOR must be 1 or 4 (got {other})"),
    }
}

fn run_1x() {
    let mut graph = SatGraph_1x::new();
    if let Some((device, config)) = open_default_output() {
        play(graph, device, config, |g, frames| {
            g.process_block(frames);
            &g.audio_out_block[..frames]
        });
    } else {
        eprintln!("No audio output device — running compute-only smoke test.");
        graph.init(48_000.0);
        graph.process_block(64);
        print_samples(&graph.audio_out_block[..16]);
    }
}

fn run_4x() {
    let mut graph = SatGraph_4x::new();
    if let Some((device, config)) = open_default_output() {
        play(graph, device, config, |g, frames| {
            g.process_block(frames);
            &g.audio_out_block[..frames]
        });
    } else {
        eprintln!("No audio output device — running compute-only smoke test.");
        graph.init(48_000.0);
        graph.process_block(64);
        print_samples(&graph.audio_out_block[..16]);
    }
}

fn open_default_output() -> Option<(cpal::Device, cpal::StreamConfig)> {
    let host = cpal::default_host();
    let device = host.default_output_device()?;
    let default_config = device.default_output_config().ok()?;
    let config = cpal::StreamConfig {
        channels: default_config.channels(),
        sample_rate: default_config.sample_rate(),
        buffer_size: cpal::BufferSize::Fixed(512),
    };
    Some((device, config))
}

fn play<G, F>(mut graph: G, device: cpal::Device, config: cpal::StreamConfig, mut process: F)
where
    G: InitGraph + Send + 'static,
    F: FnMut(&mut G, usize) -> &[f32] + Send + 'static,
{
    let sample_rate = config.sample_rate.0 as f32;
    let channels = config.channels as usize;

    // The graph itself is held inside the audio callback closure, so we
    // need to hand it (and the processing closure) into the audio thread.
    // Initialize before transferring ownership.
    graph.init_graph(sample_rate);

    let stream = device
        .build_output_stream(
            &config,
            move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                let frames = data.len() / channels;
                let block = process(&mut graph, frames);
                for (i, frame) in data.chunks_mut(channels).enumerate() {
                    let mono = block.get(i).copied().unwrap_or(0.0);
                    for sample in frame.iter_mut() {
                        *sample = mono;
                    }
                }
            },
            |err| eprintln!("audio stream error: {err}"),
            None,
        )
        .expect("failed to build output stream");

    stream.play().expect("failed to start audio stream");

    println!("Playing — press Ctrl-C to stop.");
    loop {
        thread::sleep(Duration::from_secs(1));
    }
}

/// Trait-free `init` dispatch: each variant exposes its own `init` method
/// generated by `graph!`. This shim lets `play` stay generic over `G`.
trait InitGraph {
    fn init_graph(&mut self, sample_rate: f32);
}

impl InitGraph for SatGraph_1x {
    fn init_graph(&mut self, sample_rate: f32) {
        self.init(sample_rate);
    }
}

impl InitGraph for SatGraph_4x {
    fn init_graph(&mut self, sample_rate: f32) {
        self.init(sample_rate);
    }
}

fn print_samples(samples: &[f32]) {
    print!("first {} samples: [", samples.len());
    for (i, s) in samples.iter().enumerate() {
        if i > 0 {
            print!(", ");
        }
        print!("{s:+.4}");
    }
    println!("]");
}
