use std::{
    sync::mpsc::{self, Receiver, Sender},
    thread,
    time::Duration,
};

use anyhow::{Context, Result};
use arrayvec;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use oscen::{delay::DelayEndpoints, graph, Delay, PolyBlepOscillator, PolyBlepOscillatorEndpoints};
use slint::ComponentHandle;

slint::include_modules!();

#[derive(Clone, Copy, Debug)]
enum ParamChange {
    Feedback(f32),
}

graph! {
    name: Synth;

    input value feedback = 0.0;

    output stream audio_out;

    node {
        osc = PolyBlepOscillator::sine(140.0, 0.3);
        delay = Delay::new(1.0, 0.0);
    }

    connection {
        osc.output -> delay.input;
        delay.output * feedback -> osc.phase_mod;
        osc.output -> audio_out;
    }
}

struct AudioContext {
    synth: Synth,
    channels: usize,
}

fn build_audio_context(sample_rate: f32, channels: usize) -> AudioContext {
    let mut synth = Synth::new(sample_rate);

    // Validate the graph
    if let Err(e) = synth.graph.validate() {
        eprintln!("Graph validation error: {}", e);
    }

    AudioContext { synth, channels }
}

fn audio_callback(data: &mut [f32], context: &mut AudioContext, param_rx: &Receiver<ParamChange>) {
    // Handle parameter changes
    while let Ok(change) = param_rx.try_recv() {
        match change {
            ParamChange::Feedback(value) => {
                context
                    .synth
                    .graph
                    .set_value_with_ramp(context.synth.feedback, value, 1323);
            }
        }
    }

    // Render audio
    for frame in data.chunks_mut(context.channels) {
        if let Err(err) = context.synth.graph.process() {
            eprintln!("Graph processing error: {}", err);
            for sample in frame.iter_mut() {
                *sample = 0.0;
            }
            continue;
        }

        let value = context
            .synth
            .graph
            .get_value(&context.synth.audio_out)
            .unwrap_or(0.0);

        for sample in frame.iter_mut() {
            *sample = value;
        }
    }
}

fn main() -> Result<()> {
    let (param_tx, param_rx) = mpsc::channel();

    thread::spawn(move || {
        let host = cpal::default_host();
        let device = match host.default_output_device() {
            Some(device) => device,
            None => {
                eprintln!("No output device available");
                return;
            }
        };

        let default_config = match device.default_output_config() {
            Ok(config) => config,
            Err(err) => {
                eprintln!("Failed to fetch default output config: {}", err);
                return;
            }
        };

        let config = cpal::StreamConfig {
            channels: default_config.channels(),
            sample_rate: default_config.sample_rate(),
            buffer_size: cpal::BufferSize::Fixed(512),
        };

        let sample_rate = config.sample_rate.0 as f32;
        let mut audio_context = build_audio_context(sample_rate, config.channels as usize);

        let stream = match device.build_output_stream(
            &config,
            move |data: &mut [f32], _| {
                audio_callback(data, &mut audio_context, &param_rx);
            },
            |err| eprintln!("Audio stream error: {}", err),
            None,
        ) {
            Ok(stream) => stream,
            Err(err) => {
                eprintln!("Failed to build output stream: {}", err);
                return;
            }
        };

        if let Err(err) = stream.play() {
            eprintln!("Failed to start audio stream: {}", err);
            return;
        }

        loop {
            thread::sleep(Duration::from_millis(100));
        }
    });

    run_ui(param_tx)?;
    Ok(())
}

fn run_ui(tx: Sender<ParamChange>) -> Result<()> {
    let ui = SynthWindow::new()?;

    {
        let tx = tx.clone();
        ui.on_feedback_edited(move |value| {
            let _ = tx.send(ParamChange::Feedback(value));
        });
    }

    // Set default values
    ui.set_feedback(0.0);

    ui.run().context("failed to run UI")
}
