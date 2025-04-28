# Oscen [![crates.io](https://img.shields.io/crates/v/oscen.svg)](https://crates.io/crates/oscen)

<picture>
    <source media="(prefers-color-scheme: dark)" srcset="logo-dark.svg">
    <source media="(prefers-color-scheme: light)" srcset="logo-light.svg">
    <img src="logo-light.svg">
</picture>
<br />
<br />

Oscen _[“oh-sin”]_ is a library for building modular synthesizers in Rust.

It contains a collection of components frequently used in sound synthesis
such as oscillators, filters, and envelope generators. It lets you
connect (or patch) the output of one module into the input of another.

## Example

```Rust
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use oscen::{Graph, Oscillator, TPT_Filter, OutputEndpoint};
use std::thread;

fn create_audio_graph(sample_rate: f32) -> (Graph, OutputEndpoint) {
    // Create oscen audio graph
    let mut graph = Graph::new(sample_rate);
    
    // Create oscillators and filter
    let modulator = graph.add_node(Oscillator::sine(5.0, 0.2));
    let carrier = graph.add_node(Oscillator::saw(440.0, 0.5));
    let filter = graph.add_node(TPT_Filter::new(1200.0, 0.707));
    
    // Connect nodes using the routing vec syntax
    let routing = vec![
        modulator.output() >> carrier.frequency_mod(),  // FM synthesis
        carrier.output() >> filter.input(),             // Filter the carrier
    ];
    
    // Connect all routes at once
    graph.connect_all(routing);
    
    // Return graph and the final output node
    (graph, filter.output())
}

fn main() {
    thread::spawn(move || {
        // Set up audio
        let host = cpal::default_host();
        let device = host.default_output_device().expect("no output device");
        let default_config = device.default_output_config().unwrap();
        let config = cpal::StreamConfig {
            channels: default_config.channels(),
            sample_rate: default_config.sample_rate(),
            buffer_size: cpal::BufferSize::Fixed(512),
        };
        
        let sample_rate = config.sample_rate.0 as f32;
        let channels = config.channels as usize;

        // Create audio graph
        let (mut graph, output) = create_audio_graph(sample_rate);

        // Build the audio stream
        let stream = device
            .build_output_stream(
                &config,
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    // Process audio in chunks
                    for frame in data.chunks_mut(channels) {
                        // Process the graph
                        graph.process();
                        
                        // Get the output value and write to all channels
                        if let Some(value) = graph.get_value(&output) {
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
    }).join().unwrap();
}
```
