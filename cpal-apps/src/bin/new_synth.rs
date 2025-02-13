use anyhow;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use oscen2::{Graph, Oscillator};

fn main() -> Result<(), anyhow::Error> {
    // Initialize audio
    let host = cpal::default_host();
    let device = host.default_output_device().expect("no output device");
    let config = device.default_output_config()?;
    let sample_rate = config.sample_rate().0 as f32;

    // Create audio graph
    let mut graph = Graph::new(sample_rate);

    let modulator = graph.add_node(Oscillator::sine(880.0, 0.5));
    let carrier = graph.add_node(Oscillator::sine(252.0, 0.5));

    graph.connect(modulator.signal(), carrier.frequency());

    // Set up audio stream
    let stream = device.build_output_stream(
        &config.clone().into(),
        move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
            for frame in data.chunks_mut(config.channels() as usize) {
                graph.process();
                // Get output from last node (oscillator)
                if let Some(output_key) = graph.node_outputs.values().last().and_then(|v| v.get(0))
                {
                    if let Some(&value) = graph.values.get(*output_key) {
                        // Write same value to all channels
                        for sample in frame.iter_mut() {
                            *sample = value;
                        }
                    }
                }
            }
        },
        |err| eprintln!("Audio stream error: {}", err),
        None,
    )?;

    stream.play()?;

    // Keep program running until interrupted
    println!("Playing... Press Ctrl+C to stop");
    loop {
        std::thread::sleep(std::time::Duration::from_secs(1));
    }

    Ok(())
}
