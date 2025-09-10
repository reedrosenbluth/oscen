use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use oscen::{Graph, Oscillator, TptFilter, OutputEndpoint};
use std::thread;

fn create_audio_graph(sample_rate: f32) -> (Graph, OutputEndpoint) {
    let mut graph = Graph::new(sample_rate);
    
    // Create a sine oscillator and low-pass filter
    let osc = graph.add_node(Oscillator::sine(440.0, 0.5));
    let filter = graph.add_node(TptFilter::new(1200.0, 0.707));
    
    // Connect oscillator to filter
    graph.connect(osc.output(), filter.input());
    
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
                        // Process the graph and handle potential errors
                        if let Err(e) = graph.process() {
                            eprintln!("Graph process error: {}", e);
                        }
                        
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
