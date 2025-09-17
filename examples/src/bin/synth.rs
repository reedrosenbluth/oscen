use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use eframe::egui;
use oscen::{Graph, Oscillator, OutputEndpoint, TptFilter, ValueKey};
use std::sync::mpsc::{channel, Sender};
use std::thread;

#[derive(Clone, Copy, Debug)]
struct SynthParams {
    carrier_frequency: f32,
    modulator_frequency: f32,
    cutoff_frequency: f32,
    q_factor: f32,
}

impl Default for SynthParams {
    fn default() -> Self {
        Self {
            carrier_frequency: 440.0,
            modulator_frequency: 100.0,
            cutoff_frequency: 3000.0,
            q_factor: 0.707,
        }
    }
}

struct AudioContext {
    graph: Graph,
    carrier_freq_input: ValueKey,
    modulator_freq_input: ValueKey,
    cutoff_freq_input: ValueKey,
    q_input: ValueKey,
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
        context.graph.set_value_with_ramp(
            context.carrier_freq_input,
            params.carrier_frequency,
            441,
        );
        context.graph.set_value_with_ramp(
            context.modulator_freq_input,
            params.modulator_frequency,
            441,
        );
        context
            .graph
            .set_value_with_ramp(context.cutoff_freq_input, params.cutoff_frequency, 1323);
        context
            .graph
            .set_value_with_ramp(context.q_input, params.q_factor, 441);
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
                        ui.label("Carrier Frequency");
                        if ui
                            .add(
                                egui::Slider::new(
                                    &mut self.params.carrier_frequency,
                                    20.0..=2000.0,
                                )
                                .step_by(1.0),
                            )
                            .changed()
                        {
                            let _ = self.tx.send(self.params);
                        }

                        ui.add_space(10.0);

                        // Modulator Frequency
                        ui.label("Modulator Frequency");
                        if ui
                            .add(
                                egui::Slider::new(
                                    &mut self.params.modulator_frequency,
                                    20.0..=2000.0,
                                )
                                .step_by(0.1),
                            )
                            .changed()
                        {
                            let _ = self.tx.send(self.params);
                        }
                    });
                });

                ui.add_space(20.0);

                ui.group(|ui| {
                    // ui.set_min_width(400.0);
                    ui.vertical(|ui| {
                        ui.heading("Filter");
                        ui.add_space(20.0);

                        // Filter Cutoff
                        ui.label("Filter Cutoff");
                        if ui
                            .add(
                                egui::Slider::new(
                                    &mut self.params.cutoff_frequency,
                                    20.0..=20000.0,
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
                        ui.label("Filter Q");
                        if ui
                            .add(
                                egui::Slider::new(&mut self.params.q_factor, 0.1..=10.0)
                                    .fixed_decimals(3)
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

        // initialize new graph
        let mut graph = Graph::new(sample_rate);

        // create a few nodes
        // let square = graph.add_node(Oscillator::square(0.5, 1.0));
        let modulator = graph.add_node(Oscillator::sine(100.0, 0.5));
        let carrier = graph.add_node(Oscillator::saw(440.0, 1.0));
        let filter = graph.add_node(TptFilter::new(3000.0, 0.707));

        // make connections
        // graph.connect(modulator.output(), carrier.frequency_mod());
        // graph.connect(carrier.output(), filter.input());

        // graph.connect(carrier.output(), filter.input());
        // let output = graph.combine(filter.output(), square.output(), |x, y| {
        //     if y > 0.0 {
        //         x
        //     } else {
        //         0.0
        //     }
        // });

        let routing = vec![
            // modulator.output() >> carrier.frequency_mod(),
            carrier.output() >> filter.input(),
        ];

        graph.connect_all(routing);

        let limited = graph.transform(filter.output(), |x| x.tanh());
        let output = limited;

        // create value input endpoints for the UI
        let carrier_freq_input = graph
            .insert_value_input(carrier.frequency(), 440.0)
            .expect("Failed to insert carrier frequency input");
        let modulator_freq_input = graph
            .insert_value_input(modulator.frequency(), 100.0)
            .expect("Failed to insert modulator frequency input");
        let cutoff_freq_input = graph
            .insert_value_input(filter.cutoff(), 3000.0)
            .expect("Failed to insert filter cutoff input");
        let q_input = graph
            .insert_value_input(filter.q(), 0.707)
            .expect("Failed to insert filter Q input");
        // ==========================================================

        let mut audio_context = AudioContext {
            graph,
            carrier_freq_input,
            modulator_freq_input,
            cutoff_freq_input,
            q_input,
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
        viewport: egui::ViewportBuilder::default().with_inner_size([370.0, 160.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Oscen",
        options,
        Box::new(|_cc| Ok(Box::new(ESynthApp::new(tx)))),
    )
}
