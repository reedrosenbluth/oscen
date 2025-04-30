use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use eframe::egui;
use oscen::{Graph, LP18Filter, Oscillator, OutputEndpoint, ValueKey};
use std::sync::mpsc::{channel, Sender};
use std::thread;

#[derive(Clone, Copy, Debug)]
struct SynthParams {
    frequency: f32,
    cutoff_frequency_a: f32,
    cutoff_frequency_b: f32,
    q_factor: f32,
}

impl Default for SynthParams {
    fn default() -> Self {
        Self {
            frequency: 3.0,
            cutoff_frequency_a: 1000.0,
            cutoff_frequency_b: 1900.0,
            q_factor: 0.54,
        }
    }
}

struct AudioContext {
    graph: Graph,
    oscillator_freq_input: ValueKey,
    cutoff_freq_input_a: ValueKey,
    cutoff_freq_input_b: ValueKey,
    q_input_a: ValueKey,
    q_input_b: ValueKey,
    output: OutputEndpoint,
    channels: usize,
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
        context
            .graph
            .set_value(context.oscillator_freq_input, params.frequency, 441);
        context
            .graph
            .set_value(context.cutoff_freq_input_a, params.cutoff_frequency_a, 1323);
        context
            .graph
            .set_value(context.cutoff_freq_input_b, params.cutoff_frequency_b, 1323);
        context
            .graph
            .set_value(context.q_input_a, params.q_factor, 441);
        context
            .graph
            .set_value(context.q_input_b, params.q_factor, 441);
    }

    for frame in data.chunks_mut(context.channels) {
        context.graph.process();

        if let Some(value) = context.graph.get_value(&context.output) {
            for sample in frame.iter_mut() {
                *sample = value;
            }
        }
    }
}

struct ESynthApp {
    params: SynthParams,
    tx: Sender<SynthParams>,
}

impl ESynthApp {
    fn new(tx: Sender<SynthParams>) -> Self {
        Self {
            params: SynthParams::default(),
            tx,
        }
    }
}

impl eframe::App for ESynthApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.group(|ui| {
                    // ui.set_min_width(400.0);

                    ui.vertical(|ui| {
                        ui.heading("Oscillator");
                        ui.add_space(20.0);

                        // Carrier Frequency
                        ui.label("Trigger Frequency");
                        if ui
                            .add(
                                egui::Slider::new(&mut self.params.frequency, 0.1..=10.0)
                                    .step_by(0.1),
                            )
                            .changed()
                        {
                            let _ = self.tx.send(self.params);
                        }

                        ui.add_space(10.0);
                    });
                });

                ui.add_space(20.0);

                ui.group(|ui| {
                    // ui.set_min_width(400.0);
                    ui.vertical(|ui| {
                        ui.heading("Filter");
                        ui.add_space(20.0);

                        // Filter A Cutoff
                        ui.label("Filter A Cutoff");
                        if ui
                            .add(
                                egui::Slider::new(
                                    &mut self.params.cutoff_frequency_a,
                                    20.0..=16000.0,
                                )
                                .logarithmic(true)
                                .step_by(0.1),
                            )
                            .changed()
                        {
                            let _ = self.tx.send(self.params);
                        }

                        ui.add_space(10.0);

                        // Filter B Cutoff
                        ui.label("Filter B Cutoff");
                        if ui
                            .add(
                                egui::Slider::new(
                                    &mut self.params.cutoff_frequency_b,
                                    20.0..=16000.0,
                                )
                                .logarithmic(true)
                                .step_by(0.1),
                            )
                            .changed()
                        {
                            let _ = self.tx.send(self.params);
                        }

                        ui.add_space(10.0);

                        // Filter Q
                        ui.label("Resonance (both filters)");
                        if ui
                            .add(
                                egui::Slider::new(&mut self.params.q_factor, 0.4..=0.99)
                                    .fixed_decimals(3)
                                    .logarithmic(true) // Makes small adjustments at high values more precise
                                    .step_by(0.001),
                            )
                            .changed()
                        {
                            let _ = self.tx.send(self.params);
                        }
                    });
                });
            });
        });
    }
}

fn main() -> Result<(), eframe::Error> {
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

        // ==========================================================
        // Construct Audio Graph
        // ==========================================================

        let default_params = SynthParams::default();
        let mut graph = Graph::new(sample_rate);

        // Use an even narrower pulse for sharper excitation of the filters
        let pulse_osc = graph.add_node(Oscillator::new(
            default_params.frequency,
            1.0,
            |p| if p < 0.001 { 1.0 } else { 0.0 }, // Stronger, narrower pulse
        ));

        let sine = graph.add_node(Oscillator::sine(1000.0, 70.0));

        let filter_a = graph.add_node(LP18Filter::new(
            default_params.cutoff_frequency_a,
            default_params.q_factor,
        ));
        let filter_b = graph.add_node(LP18Filter::new(
            default_params.cutoff_frequency_b,
            default_params.q_factor,
        ));

        graph.connect(pulse_osc.output(), filter_a.input());
        graph.connect(pulse_osc.output(), filter_b.input());

        // graph.connect(sine.output(), filter_a.fmod());
        // graph.connect(sine.output(), filter_b.fmod());

        // Use transform to create a sequencer that advances when pulse goes high
        let sequencer = graph.transform(pulse_osc.output(), |pulse_value: f32| -> f32 {
            static SEQ_VALUES: [f32; 3] = [200., 400., 800.];
            static mut SEQ_INDEX: usize = 0;
            static mut PREV_PULSE: f32 = 0.0;

            unsafe {
                if pulse_value > 0.5 && PREV_PULSE <= 0.5 {
                    SEQ_INDEX = (SEQ_INDEX + 1) % SEQ_VALUES.len();
                }

                PREV_PULSE = pulse_value;
                SEQ_VALUES[SEQ_INDEX]
            }
        });

        graph.connect(sequencer, filter_a.fmod());
        graph.connect(sequencer, filter_b.fmod());

        let diff = graph.combine(filter_a.output(), filter_b.output(), |x, y| x - y);

        // Apply tanh limiting to prevent filter feedback from getting out of control
        let limited = graph.transform(diff, |x| x.tanh());
        let output = limited;

        // create value input endpoints for the UI
        let oscillator_freq_input = graph
            .insert_value_input(pulse_osc.frequency(), default_params.frequency)
            .expect("Failed to insert carrier frequency input");

        let cutoff_freq_input_a = graph
            .insert_value_input(filter_a.cutoff(), default_params.cutoff_frequency_a)
            .expect("Failed to insert filter A cutoff input");

        let cutoff_freq_input_b = graph
            .insert_value_input(filter_b.cutoff(), default_params.cutoff_frequency_b)
            .expect("Failed to insert filter B cutoff input");

        let q_input_a = graph
            .insert_value_input(filter_a.resonance(), default_params.q_factor)
            .expect("Failed to insert filter A Q input");

        let q_input_b = graph
            .insert_value_input(filter_b.resonance(), default_params.q_factor)
            .expect("Failed to insert filter B Q input");
        // ==========================================================

        let mut audio_context = AudioContext {
            graph,
            oscillator_freq_input,
            cutoff_freq_input_a,
            cutoff_freq_input_b,
            q_input_a,
            q_input_b,
            output,
            channels: config.channels as usize,
        };

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

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([370.0, 220.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Oscen",
        options,
        Box::new(|_cc| Ok(Box::new(ESynthApp::new(tx)))),
    )
}
