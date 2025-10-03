use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use oscen::{
    Graph, InputEndpoint, Node, NodeKey, OutputEndpoint, PolyBlepOscillator, ProcessingContext,
    ProcessingNode, SignalProcessor, TptFilter, Value, ValueInputHandle, ValueKey,
};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::mpsc::{channel, Sender};
use std::thread;

use slint::ComponentHandle;

slint::include_modules!();

const NUM_OSCILLATORS: usize = 7;
const DETUNE_OFFSETS: [f32; NUM_OSCILLATORS] = [-3.0, -2.0, -1.0, 0.0, 1.0, 2.0, 3.0];
const DETUNE_STEP_CENTS: f32 = 6.0;

#[derive(Debug, Node)]
struct DetuneFrequency {
    #[input(value)]
    base_frequency: f32,
    #[input(value)]
    spread: f32,

    #[output(value)]
    frequency: f32,

    offset_steps: f32,
}

impl DetuneFrequency {
    fn new(offset_steps: f32) -> Self {
        Self {
            base_frequency: 0.0,
            spread: 0.0,
            frequency: 0.0,
            offset_steps,
        }
    }
}

impl SignalProcessor for DetuneFrequency {
    fn process<'a>(&mut self, _sample_rate: f32, context: &mut ProcessingContext<'a>) -> f32 {
        let base = self.get_base_frequency(context).max(0.0);
        let spread = self.get_spread(context).clamp(0.0, 1.0);
        let cents = self.offset_steps * spread * DETUNE_STEP_CENTS;
        let ratio = 2f32.powf(cents / 1200.0);
        self.frequency = base * ratio;
        self.frequency
    }
}

#[derive(Clone, Copy, Debug)]
struct SynthParams {
    carrier_frequency: f32,
    cutoff_frequency: f32,
    q_factor: f32,
    volume: f32,
    spread: f32,
}

impl Default for SynthParams {
    fn default() -> Self {
        Self {
            carrier_frequency: 440.0,
            cutoff_frequency: 3000.0,
            q_factor: 0.707,
            volume: 0.8,
            spread: 0.0,
        }
    }
}

struct AudioContext {
    graph: Graph,
    base_freq_input: ValueInputHandle,
    spread_input: ValueInputHandle,
    cutoff_freq_input: ValueInputHandle,
    q_input: ValueInputHandle,
    volume_input: ValueInputHandle,
    output: OutputEndpoint,
    channels: usize,
}

fn build_audio_context(sample_rate: f32, channels: usize) -> AudioContext {
    let mut graph = Graph::new(sample_rate);

    let base_frequency = graph.add_node(Value::new(440.0));
    let spread = graph.add_node(Value::new(0.0));

    let mut summed_osc_output: Option<OutputEndpoint> = None;
    let osc_amplitude = 1.0 / NUM_OSCILLATORS as f32;

    for &offset_steps in DETUNE_OFFSETS.iter() {
        let detune = graph.add_node(DetuneFrequency::new(offset_steps));
        graph.connect(base_frequency.output(), detune.base_frequency());
        graph.connect(spread.output(), detune.spread());

        let osc = graph.add_node(PolyBlepOscillator::saw(440.0, osc_amplitude));
        graph.connect(detune.frequency(), osc.frequency());

        let osc_output = osc.output();
        summed_osc_output = Some(match summed_osc_output {
            Some(accum) => graph.combine(accum, osc_output, |a, b| a + b),
            None => osc_output,
        });
    }

    let filter = graph.add_node(TptFilter::new(3000.0, 0.707));
    let volume = graph.add_node(Value::new(0.4));

    let summed_osc_output = summed_osc_output.expect("No oscillators were created");

    graph.connect(summed_osc_output, filter.input());

    let output = graph.combine(filter.output(), volume.output(), |x, v| x * v);

    // if graph
    //     .insert_value_input(base_frequency.input(), 440.0)
    //     .is_none()
    // {
    //     panic!("Failed to insert base frequency input");
    // }
    // if graph.insert_value_input(spread.input(), 0.0).is_none() {
    //     panic!("Failed to insert spread input");
    // }
    // if graph.insert_value_input(filter.cutoff(), 3000.0).is_none() {
    //     panic!("Failed to insert filter cutoff input");
    // }
    // if graph.insert_value_input(filter.q(), 0.707).is_none() {
    //     panic!("Failed to insert filter Q input");
    // }
    // if graph.insert_value_input(volume.input(), 0.4).is_none() {
    //     panic!("Failed to insert volume input");
    // }

    AudioContext {
        graph,
        base_freq_input: base_frequency.input(),
        spread_input: spread.input(),
        cutoff_freq_input: filter.cutoff(),
        q_input: filter.q(),
        volume_input: volume.input(),
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
            (
                context.base_freq_input,
                params.carrier_frequency.max(0.0),
                441,
            ),
            (context.spread_input, params.spread.clamp(0.0, 1.0), 441),
            (context.cutoff_freq_input, params.cutoff_frequency, 1323),
            (context.q_input, params.q_factor, 441),
            (context.volume_input, params.volume, 441),
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
    wire_knob!(
        |state: &mut SynthParams, value| state.volume = value,
        SynthWindow::on_volume_edited
    );
    wire_knob!(
        |state: &mut SynthParams, value| state.spread = value,
        SynthWindow::on_spread_edited
    );

    ui.set_carrier_frequency(440.0);
    ui.set_cutoff_frequency(3000.0);
    ui.set_q_factor(0.707);
    ui.set_volume(0.8);
    ui.set_spread(0.0);

    ui.run()
}
