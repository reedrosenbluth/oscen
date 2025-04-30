use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use eframe::egui;
use oscen::{Graph, LP18Filter, Oscillator, OutputEndpoint, ValueKey};
use std::sync::mpsc::{channel, Sender};
use std::thread;

#[derive(Clone, Copy, Debug)]
struct SynthParams {
    carrier_frequency: f32,
    modulator_frequency: f32,
    cutoff_frequency: f32,
    resonance: f32,
}

impl Default for SynthParams {
    fn default() -> Self {
        Self {
            carrier_frequency: 110.0,
            modulator_frequency: 0.2,
            cutoff_frequency: 1200.0,
            resonance: 0.4,
        }
    }
}

struct AudioContext {
    graph: Graph,
    carrier_freq_input: ValueKey,
    modulator_freq_input: ValueKey,
    cutoff_freq_input: ValueKey,
    resonance_input: ValueKey,
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
        context.graph.set_value(context.carrier_freq_input, params.carrier_frequency, 441);
        context.graph.set_value(context.modulator_freq_input, params.modulator_frequency, 441);
        context.graph.set_value(context.cutoff_freq_input, params.cutoff_frequency, 1323);
        context.graph.set_value(context.resonance_input, params.resonance, 441);
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

struct LP18SynthApp {
    params: SynthParams,
    tx: Sender<SynthParams>,
}

impl LP18SynthApp {
    fn new(tx: Sender<SynthParams>) -> Self {
        Self {
            params: SynthParams::default(),
            tx,
        }
    }
}

impl eframe::App for LP18SynthApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.group(|ui| {
                    ui.vertical(|ui| {
                        ui.heading("Oscillator");
                        ui.add_space(20.0);

                        // Carrier Frequency
                        ui.label("Carrier Frequency");
                        if ui
                            .add(
                                egui::Slider::new(
                                    &mut self.params.carrier_frequency,
                                    20.0..=1000.0,
                                )
                                .logarithmic(true)
                                .step_by(0.1),
                            )
                            .changed()
                        {
                            let _ = self.tx.send(self.params);
                        }

                        ui.add_space(10.0);

                        // LFO Frequency
                        ui.label("LFO Frequency");
                        if ui
                            .add(
                                egui::Slider::new(
                                    &mut self.params.modulator_frequency,
                                    0.05..=10.0,
                                )
                                .logarithmic(true)
                                .step_by(0.01),
                            )
                            .changed()
                        {
                            let _ = self.tx.send(self.params);
                        }
                    });
                });

                ui.add_space(20.0);

                ui.group(|ui| {
                    ui.vertical(|ui| {
                        ui.heading("LP18 Filter");
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

                        // Resonance
                        ui.label("Resonance");
                        if ui
                            .add(
                                egui::Slider::new(&mut self.params.resonance, 0.0..=0.9)
                                    .fixed_decimals(2)
                                    .step_by(0.01),
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

        println!("Initializing audio graph...");
        println!("Sample rate: {}", sample_rate);

        // Initialize new graph
        let mut graph = Graph::new(sample_rate);

        // Source oscillator - single sawtooth
        let carrier = graph.add_node(Oscillator::saw(110.0, 0.5));
        println!("Created carrier oscillator");

        // LFO for filter modulation
        let lfo = graph.add_node(Oscillator::sine(0.2, 0.6));
        println!("Created LFO");

        // LP18 filter with moderate cutoff and resonance
        let filter = graph.add_node(LP18Filter::new(1200.0, 0.4));
        println!("Created LP18 filter");

        // Connect oscillator to filter - DIRECT CONNECTION LIKE MINIMAL EXAMPLE
        graph.connect(carrier.output(), filter.audio_in());
        println!("Connected carrier to filter input");

        // Connect LFO to filter cutoff
        graph.connect(lfo.output(), filter.cutoff());
        println!("Connected LFO to filter cutoff");

        // No need for separate resonance oscillator or feedback path
        // The LP18Filter includes the res_in input to allow for external feedback
        println!("Connected resonance feedback path");

        // Get filter output
        let filter_out = filter.audio_out();
        println!("Set up output from filter.audio_out()");

        // Apply tanh limiting for soft clipping (just like in the working synth.rs)
        let limited = graph.transform(filter_out, |x| x.tanh());
        let output = limited;

        // Create value input endpoints for the UI controls
        let carrier_freq_input = graph
            .insert_value_input(carrier.frequency(), 110.0)
            .expect("Failed to insert carrier frequency input");
        let lfo_freq_input = graph
            .insert_value_input(lfo.frequency(), 0.2)
            .expect("Failed to insert LFO frequency input");
        let cutoff_freq_input = graph
            .insert_value_input(filter.cutoff(), 1200.0)
            .expect("Failed to insert filter cutoff input");
        let resonance_input = graph
            .insert_value_input(filter.resonance(), 0.3)
            .expect("Failed to insert resonance input");
        // ==========================================================

        let mut audio_context = AudioContext {
            graph,
            carrier_freq_input,
            modulator_freq_input: lfo_freq_input,
            cutoff_freq_input,
            resonance_input,
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
        viewport: egui::ViewportBuilder::default().with_inner_size([420.0, 180.0]),
        ..Default::default()
    };

    eframe::run_native(
        "LP18 Filter Synth",
        options,
        Box::new(|_cc| Ok(Box::new(LP18SynthApp::new(tx)))),
    )
}
