use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use oscen::prelude::*;
use std::thread;

graph! {
    name: SynthGraph;

    output stream out;

    nodes {
        osc = PolyBlepOscillator::saw(440.0, 0.6);
        filter = TptFilter::new(4000.0, 0.707);
    }

    connections {
        osc.output -> filter.input;
        filter.output -> out;
    }
}

fn main() {
    thread::spawn(move || {
        // Set up audio
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

        // Create audio graph
        let mut graph = SynthGraph::new();
        graph.init(sample_rate);
        let mut counter = 0;

        // Build the audio stream
        let stream = device
            .build_output_stream(
                &config,
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    // Process audio in chunks
                    for frame in data.chunks_mut(channels) {
                        // Process the graph
                        counter += 1;
                        if counter >= 48000 {
                            let start = std::time::Instant::now();
                            graph.process();
                            println!(
                                "simple_synth/process    time:   [{} ns]",
                                start.elapsed().as_nanos()
                            );
                            counter = 0;
                        } else {
                            graph.process();
                        }

                        // Get the output value and write to all channels
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

        // Start playback
        stream.play().unwrap();

        // Keep the thread alive
        loop {
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
    })
    .join()
    .unwrap();
}
