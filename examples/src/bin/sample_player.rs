//! Realtime looping sample player with live file swapping and an LFO-swept
//! resonant lowpass filter.
//!
//! Pass one or more WAV files at the OUTPUT DEVICE's sample rate (the asset
//! loader does not resample — a mismatch is reported and that file skipped).
//! The player loops the first file; every 4 seconds it swaps to the next file
//! in the list, decoded and published from the control thread through a
//! lock-free handoff so the audio thread never decodes, allocates, or frees.
//!
//! Usage:
//!   cargo run -p oscen-examples --bin sample_player -- <a.wav> [b.wav ...]

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use oscen::asset::{AssetEndpoint, AssetLoadHandle};
use oscen::prelude::*;
use std::time::Duration;

graph! {
    name: SamplePlayerGraph;

    output stream out;

    external sample: AudioAsset;

    nodes {
        player = SamplePlayer::new();
        lfo = PolyBlepOscillator::sine(0.3, 1.0);
        filter = TptFilter::new(800.0, 0.7);
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
    let dummy = AssetLoadHandle::new(dummy_pub, SamplePlayer::asset_builder());
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
                for s in frame.iter_mut() {
                    *s = graph.out;
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
