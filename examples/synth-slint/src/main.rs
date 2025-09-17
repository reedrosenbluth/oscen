use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use oscen::{Graph, OutputEndpoint, PolyBlepOscillator, TptFilter, ValueKey};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::mpsc::{channel, Sender};
use std::thread;

use slint::ComponentHandle;

slint::include_modules!();

#[derive(Clone, Copy, Debug)]
struct SynthParams {
    carrier_frequency: f32,
    cutoff_frequency: f32,
    q_factor: f32,
}

impl Default for SynthParams {
    fn default() -> Self {
        Self {
            carrier_frequency: 440.0,
            cutoff_frequency: 3000.0,
            q_factor: 0.707,
        }
    }
}

struct AudioContext {
    graph: Graph,
    osc_freq_input: ValueKey,
    cutoff_freq_input: ValueKey,
    q_input: ValueKey,
    output: OutputEndpoint,
    channels: usize,
}

fn build_audio_context(sample_rate: f32, channels: usize) -> AudioContext {
    let mut graph = Graph::new(sample_rate);

    let osc = graph.add_node(PolyBlepOscillator::square(440.0, 1.0));
    let filter = graph.add_node(TptFilter::new(3000.0, 0.707));

    graph.connect(osc.output(), filter.input());

    let output = graph.transform(filter.output(), |x| x * 0.5);

    let osc_freq_input = graph
        .insert_value_input(osc.frequency(), 440.0)
        .expect("Failed to insert carrier frequency input");
    let cutoff_freq_input = graph
        .insert_value_input(filter.cutoff(), 3000.0)
        .expect("Failed to insert filter cutoff input");
    let q_input = graph
        .insert_value_input(filter.q(), 0.707)
        .expect("Failed to insert filter Q input");

    AudioContext {
        graph,
        osc_freq_input,
        cutoff_freq_input,
        q_input,
        output,
        channels,
    }
}

fn audio_callback(
    data: &mut [f32],
    context: &mut AudioContext,
    rx: &std::sync::mpsc::Receiver<SynthParams>,
) {
    let mut latest_params = None;
    while let Ok(params) = rx.try_recv() {
        latest_params = Some(params);
    }

    if let Some(params) = latest_params {
        let updates = [
            (context.osc_freq_input, params.carrier_frequency, 441),
            (context.cutoff_freq_input, params.cutoff_frequency, 1323),
            (context.q_input, params.q_factor, 441),
        ];

        for (key, value, ramp) in updates {
            context.graph.set_value_with_ramp(key, value, ramp);
        }
    }

    for frame in data.chunks_mut(context.channels) {
        let _ = context.graph.process();

        if let Some(value) = context.graph.get_value(&context.output) {
            for sample in frame.iter_mut() {
                *sample = value;
            }
        }
    }
}

fn main() -> Result<(), slint::PlatformError> {
    let (tx, rx) = channel();

    thread::spawn(move || {
        let host = cpal::default_host();
        let device = host.default_output_device().expect("no output device");
        let default_config = device.default_output_config().unwrap();
        let config = cpal::StreamConfig {
            channels: default_config.channels(),
            sample_rate: default_config.sample_rate(),
            buffer_size: cpal::BufferSize::Fixed(512),
        };

        let sample_rate = config.sample_rate.0 as f32;

        let mut audio_context = build_audio_context(sample_rate, config.channels as usize);

        let stream = device
            .build_output_stream(
                &config.clone(),
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    audio_callback(data, &mut audio_context, &rx);
                },
                |err| eprintln!("Audio stream error: {}", err),
                None,
            )
            .unwrap();

        stream.play().unwrap();
        loop {
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    });

    run_ui(tx)
}

fn run_ui(tx: Sender<SynthParams>) -> Result<(), slint::PlatformError> {
    let ui = SynthWindow::new()?;

    let params_state = Rc::new(RefCell::new(SynthParams::default()));

    macro_rules! wire_knob {
        ($setter:expr, $register:expr) => {{
            let params = params_state.clone();
            let tx = tx.clone();
            $register(&ui, move |value| {
                let mut state = params.borrow_mut();
                $setter(&mut state, value);
                let _ = tx.send(*state);
            });
        }};
    }

    wire_knob!(
        |state: &mut SynthParams, value| state.carrier_frequency = value,
        SynthWindow::on_carrier_frequency_edited
    );
    wire_knob!(
        |state: &mut SynthParams, value| state.cutoff_frequency = value,
        SynthWindow::on_cutoff_frequency_edited
    );
    wire_knob!(
        |state: &mut SynthParams, value| state.q_factor = value,
        SynthWindow::on_q_factor_edited
    );

    ui.set_carrier_frequency(440.0);
    ui.set_cutoff_frequency(3000.0);
    ui.set_q_factor(0.707);

    ui.run()
}
