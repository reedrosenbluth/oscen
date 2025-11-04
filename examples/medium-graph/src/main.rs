use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use oscen::{Graph, IirLowpass, Oscillator, PolyBlepOscillator, StreamOutput, ValueParam};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::mpsc::{channel, Sender};
use std::thread;

use slint::ComponentHandle;

slint::include_modules!();

#[derive(Clone, Copy, Debug)]
struct SynthParams {
    sine_frequency: f32,
    saw_frequency: f32,
    cutoff_frequency: f32,
    q_factor: f32,
    volume: f32,
}

impl Default for SynthParams {
    fn default() -> Self {
        Self {
            sine_frequency: 440.0,
            saw_frequency: 442.0,
            cutoff_frequency: 1000.0,
            q_factor: 0.7,
            volume: 0.5,
        }
    }
}

struct AudioContext {
    graph: Graph,
    sine_freq_param: ValueParam,
    saw_freq_param: ValueParam,
    cutoff_param: ValueParam,
    q_param: ValueParam,
    volume_param: ValueParam,
    output: StreamOutput,
    channels: usize,
}

fn build_audio_context(sample_rate: f32, channels: usize) -> AudioContext {
    let mut graph = Graph::new(sample_rate);

    // Create value parameters
    let sine_freq_param = graph.value_param(440.0);
    let saw_freq_param = graph.value_param(442.0);
    let cutoff_param = graph.value_param(1000.0);
    let q_param = graph.value_param(0.7);
    let volume_param = graph.value_param(0.5);

    // Create oscillators
    let sine_osc = graph.add_node(Oscillator::sine(440.0, 0.2));
    let saw_osc = graph.add_node(PolyBlepOscillator::saw(442.0, 0.2));

    // Connect frequency parameters
    graph.connect_all(vec![
        sine_freq_param >> sine_osc.frequency,
        saw_freq_param >> saw_osc.frequency,
    ]);

    // Mix the two oscillators
    let mixed = graph.combine(sine_osc.output, saw_osc.output, |a, b| a + b);

    // Create filter
    let filter = graph.add_node(IirLowpass::new(1000.0, 0.7));
    graph.connect_all(vec![
        mixed >> filter.input,
        cutoff_param >> filter.cutoff,
        q_param >> filter.q,
    ]);

    // Apply volume to filtered signal
    let output = graph.combine(filter.output, volume_param, |signal, vol| signal * vol);

    AudioContext {
        graph,
        sine_freq_param,
        saw_freq_param,
        cutoff_param,
        q_param,
        volume_param,
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
            (context.sine_freq_param, params.sine_frequency.max(0.0), 441),
            (context.saw_freq_param, params.saw_frequency.max(0.0), 441),
            (context.cutoff_param, params.cutoff_frequency.max(20.0), 441),
            (context.q_param, params.q_factor.clamp(0.1, 10.0), 441),
            (context.volume_param, params.volume.clamp(0.0, 1.0), 441),
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
    let ui = MediumGraphWindow::new()?;

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

    // Wire up oscillator controls
    wire_knob!(
        |state: &mut SynthParams, value| state.sine_frequency = value,
        MediumGraphWindow::on_sine_frequency_edited
    );
    wire_knob!(
        |state: &mut SynthParams, value| state.saw_frequency = value,
        MediumGraphWindow::on_saw_frequency_edited
    );

    // Wire up filter controls
    wire_knob!(
        |state: &mut SynthParams, value| state.cutoff_frequency = value,
        MediumGraphWindow::on_cutoff_frequency_edited
    );
    wire_knob!(
        |state: &mut SynthParams, value| state.q_factor = value,
        MediumGraphWindow::on_q_factor_edited
    );

    // Wire up volume control
    wire_knob!(
        |state: &mut SynthParams, value| state.volume = value,
        MediumGraphWindow::on_volume_edited
    );

    // Set initial values
    ui.set_sine_frequency(440.0);
    ui.set_saw_frequency(442.0);
    ui.set_cutoff_frequency(1000.0);
    ui.set_q_factor(0.7);
    ui.set_volume(0.5);

    ui.run()
}
