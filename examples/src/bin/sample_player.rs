//! Realtime looping **stereo** sample player with live file swapping and an
//! LFO-swept resonant lowpass filter applied independently per channel.
//!
//! Pass one or more WAV files; the asset loader resamples each to the OUTPUT
//! DEVICE's sample rate on load (a file that fails to decode is skipped).
//! The player loops the first file; every 4 seconds it swaps to the next file
//! in the list, decoded and published from the control thread through a
//! lock-free handoff so the audio thread never decodes, allocates, or frees.
//!
//! The signal path is `Frame<2>` end to end: a stereo `SamplePlayer` feeds a
//! stereo `TptFilter` (one integrator state per channel, shared cutoff/Q swept
//! by a mono LFO). The graph declares a frame-typed top-level
//! `output stream out: Frame<2>;`, so the audio callback reads the stereo result
//! straight from `graph.out` per channel.
//!
//! Usage:
//!   cargo run -p oscen-examples --bin sample_player -- <a.wav> [b.wav ...]

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use oscen::asset::{AssetEndpoint, AssetLoadHandle};
use oscen::prelude::*;
use std::time::Duration;

graph! {
    name: SamplePlayerGraph;

    external sample: AudioAsset;
    output stream out: Frame<2>;

    nodes {
        player = SamplePlayer::<Frame<2>>::new();
        lfo = PolyBlepOscillator::sine(0.3, 1.0);
        filter = TptFilter::<Frame<2>>::new(800.0, 0.7);
    }

    connections {
        sample -> player.buf;
        player.output -> filter.input;
        lfo.output -> filter.f_mod;
        filter.output -> out;
    }
}

fn main() -> anyhow::Result<()> {
    let files: Vec<String> = std::env::args().skip(1).collect();
    if files.is_empty() {
        eprintln!("usage: sample_player <a.wav> [b.wav ...]");
        std::process::exit(2);
    }

    let host = cpal::default_host();
    let device = host.default_output_device().expect("no output device");
    let default_config = device.default_output_config().unwrap();
    let config = cpal::StreamConfig {
        channels: 2,
        sample_rate: default_config.sample_rate(),
        buffer_size: cpal::BufferSize::Fixed(512),
    };
    let sample_rate = config.sample_rate.0 as f32;
    let channels = config.channels as usize;
    println!("output @ {sample_rate} Hz — WAV files must match this rate");

    let mut graph = SamplePlayerGraph::new();
    graph.init(sample_rate);

    // Move the load handle to the control thread (see plan: control/audio split).
    let (dummy_pub, _dummy_con) = oscen::handoff::pair();
    let dummy = AssetLoadHandle::new(dummy_pub, SamplePlayer::<Frame<2>>::asset_builder());
    let mut loader = std::mem::replace(&mut graph.sample, dummy);
    loader.set_graph_rate(config.sample_rate.0);

    // Start with sound: load the first file before the stream opens.
    if let Err(e) = loader.load_wav(&files[0]) {
        eprintln!("failed to load {}: {e}", files[0]);
        std::process::exit(1);
    }
    println!("playing {}", files[0]);

    let stream = device.build_output_stream(
        &config,
        move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
            for frame in data.chunks_mut(channels) {
                graph.process();
                // Interleave the stereo sink across the output channels; clamp
                // to the available source channels for non-stereo devices.
                for (ch, s) in frame.iter_mut().enumerate() {
                    *s = graph.out.0[ch.min(1)];
                }
            }
        },
        |err| eprintln!("audio stream error: {err}"),
        None,
    )?;
    stream.play()?;

    // Control loop: cycle the file list, swapping live.
    let mut idx = 0usize;
    loop {
        std::thread::sleep(Duration::from_secs(4));
        idx = (idx + 1) % files.len();
        match loader.load_wav(&files[idx]) {
            Ok(()) => println!("swapped to {}", files[idx]),
            Err(e) => eprintln!("skip {} ({e})", files[idx]),
        }
    }
}
