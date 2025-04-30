use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use oscen::{Graph, LP18Filter, Oscillator};
use std::thread;
use std::time::Duration;

fn main() {
    println!("Starting LP18 minimal synth example...");
    println!("You should hear a filtered sawtooth wave at 220Hz");
    println!("Press Ctrl+C to exit");

    // Start audio thread
    thread::spawn(move || {
        // Get default audio device
        let host = cpal::default_host();
        let device = host.default_output_device().expect("no output device");
        let default_config = device.default_output_config().unwrap();

        // Create stream configuration
        let config = cpal::StreamConfig {
            channels: default_config.channels(),
            sample_rate: default_config.sample_rate(),
            buffer_size: cpal::BufferSize::Fixed(512),
        };

        let sample_rate = config.sample_rate.0 as f32;
        let channels = config.channels as usize;

        println!("Audio configuration:");
        println!("  Sample rate: {}", sample_rate);
        println!("  Channels: {}", channels);

        // Create audio graph with minimal components - exactly like our test
        let mut graph = Graph::new(sample_rate);

        // Sawtooth at 220Hz with amplitude 0.5
        let saw = graph.add_node(Oscillator::saw(220.0, 0.5));

        // LP18 filter with fixed cutoff and no resonance
        let filter = graph.add_node(LP18Filter::new(1200.0, 0.0));

        // Direct connection
        graph.connect(saw.output(), filter.audio_in());

        // Build audio stream
        let stream = device
            .build_output_stream(
                &config,
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    // Process each frame
                    for frame in data.chunks_mut(channels) {
                        // Process the graph
                        graph.process();

                        // Get output value directly from filter
                        if let Some(value) = graph.get_value(&filter.audio_out()) {
                            // Apply to all channels with some attenuation to avoid clipping
                            let output = value * 0.5;
                            for sample in frame.iter_mut() {
                                *sample = output;
                            }
                        }
                    }
                },
                |err| eprintln!("Audio stream error: {}", err),
                None,
            )
            .unwrap();

        // Start playback
        println!("Starting audio playback...");
        stream.play().unwrap();

        // Keep the thread alive
        loop {
            thread::sleep(Duration::from_millis(100));
        }
    });

    // Keep main thread alive
    loop {
        thread::sleep(Duration::from_secs(1));
    }
}
