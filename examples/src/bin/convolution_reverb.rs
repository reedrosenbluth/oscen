//! Convolution reverb: a saw oscillator run through a `Convolver` node with
//! a synthetic impulse response (a unit dry tap followed by an exponentially
//! decaying noise tail).
//!
//! The dry tap at sample 0 also demonstrates the convolver's zero latency:
//! the dry signal arrives at the output on the same sample it goes in.

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use oscen::prelude::*;
use std::thread;

/// Sample rate the impulse response is generated for. The `graph!` macro
/// constructs nodes before the device rate is known, so the example assumes
/// the common default; on devices running at another rate the tail simply
/// decays proportionally faster or slower.
const IR_SAMPLE_RATE: f32 = 48_000.0;

/// A synthetic reverb impulse response: unit dry tap, then 1.5 seconds of
/// exponentially decaying noise.
fn reverb_ir() -> Vec<f32> {
    let len = (IR_SAMPLE_RATE * 1.5) as usize;
    let mut state = 0x9E37_79B9_7F4A_7C15_u64;
    let mut ir: Vec<f32> = (0..len)
        .map(|i| {
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let noise = ((state >> 33) as f32 / (u32::MAX >> 1) as f32) - 1.0;
            let seconds = i as f32 / IR_SAMPLE_RATE;
            noise * (-seconds * 4.0).exp() * 0.03
        })
        .collect();
    ir[0] = 1.0;
    ir
}

graph! {
    name: ReverbGraph;

    output stream out;

    nodes {
        osc = PolyBlepOscillator::saw(220.0, 0.2);
        reverb = Convolver::new(reverb_ir());
    }

    connections {
        osc.output -> reverb.input;
        reverb.output -> out;
    }
}

fn main() {
    thread::spawn(move || {
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

        let mut graph = ReverbGraph::new();
        graph.init(sample_rate);

        let stream = device
            .build_output_stream(
                &config,
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    for frame in data.chunks_mut(channels) {
                        graph.process();
                        if let Some(value) = graph.get_stream_output(0) {
                            for sample in frame.iter_mut() {
                                *sample = value;
                            }
                        }
                    }
                },
                |err| eprintln!("Audio stream error: {}", err),
                None,
            )
            .unwrap();

        stream.play().unwrap();

        loop {
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
    })
    .join()
    .unwrap();
}
