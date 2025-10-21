use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use oscen::{
    Graph, InputEndpoint, Node, NodeKey, PolyBlepOscillator, ProcessingContext, ProcessingNode,
    SignalProcessor, StreamOutput, TptFilter, ValueKey,
};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::mpsc::{channel, Sender};
use std::thread;

use slint::ComponentHandle;

slint::include_modules!();

const NUM_OSCILLATORS: usize = 5;
const DETUNE_OFFSETS: [f32; NUM_OSCILLATORS] = [-4.0, -2.0, 0.0, 2.0, 4.0];
const DETUNE_STEP_CENTS: f32 = 300.0;

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

use oscen::ValueParam;

struct AudioContext {
    graph: Graph,
    base_freq_param: ValueParam,
    spread_param: ValueParam,
    cutoff_param: ValueParam,
    q_param: ValueParam,
    volume_param: ValueParam,
    output: StreamOutput,
    channels: usize,
}

fn build_audio_context(sample_rate: f32, channels: usize) -> AudioContext {
    let mut graph = Graph::new(sample_rate);

    let base_param = graph.value_param(440.0);
    let spread_param = graph.value_param(0.0);
    let cutoff_param = graph.value_param(3000.0);
    let q_param = graph.value_param(0.707);
    let volume_param = graph.value_param(0.4);

    let mut summed_osc_output: Option<StreamOutput> = None;
    let osc_amplitude = 1.0 / NUM_OSCILLATORS as f32;

    for &offset_steps in DETUNE_OFFSETS.iter() {
        let detune = graph.add_node(DetuneFrequency::new(offset_steps));
        let osc = graph.add_node(PolyBlepOscillator::saw(440.0, osc_amplitude));

        graph.connect_all(vec![
            base_param >> detune.base_frequency,
            spread_param >> detune.spread,
            detune.frequency >> osc.frequency,
        ]);

        let osc_output = osc.output;
        summed_osc_output = Some(match summed_osc_output {
            Some(accum) => graph.combine(accum, osc_output, |a, b| a + b),
            None => osc_output,
        });
    }

    let filter = graph.add_node(TptFilter::new(3000.0, 0.707));
    let summed_osc_output = summed_osc_output.expect("No oscillators were created");

    graph.connect_all(vec![
        cutoff_param >> filter.cutoff,
        q_param >> filter.q,
        summed_osc_output >> filter.input,
    ]);

    let output = graph.combine(filter.output, volume_param, |x, v| x * v);

    AudioContext {
        graph,
        base_freq_param: base_param,
        spread_param: spread_param,
        cutoff_param: cutoff_param,
        q_param: q_param,
        volume_param: volume_param,
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
                context.base_freq_param,
                params.carrier_frequency.max(0.0),
                441,
            ),
            (context.spread_param, params.spread.clamp(0.0, 1.0), 441),
            (context.cutoff_param, params.cutoff_frequency, 1323),
            (context.q_param, params.q_factor, 441),
            (context.volume_param, params.volume, 441),
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
