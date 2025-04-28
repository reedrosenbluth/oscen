use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use eframe::egui;
// use oscen::{
//     EndpointDefinition, EndpointMetadata, Graph, InputEndpoint, NodeKey, OutputEndpoint,
//     ProcessingNode, SignalProcessor, ValueKey,
// };
use oscen::graph::*;
use oscen::Node;
use std::f32::consts::PI;
use std::sync::mpsc::{channel, Sender};
use std::thread;

// Synth Params
#[derive(Clone, Copy, Debug)]
struct SynthParams {
    frequency: f32,
}

impl Default for SynthParams {
    fn default() -> Self {
        Self { frequency: 440.0 }
    }
}
// Oscillator
#[derive(Debug, Node)]
pub struct SVSine {
    #[input]
    frequency: f32,

    x: f32,
    w_1: f32,

    #[output]
    output: f32,
}

impl SVSine {
    pub fn new(frequency: f32, amplitude: f32) -> Self {
        Self {
            frequency,
            output: 0.0,
            x: 1.0,
            w_1: 0.0,
        }
    }
}

impl SignalProcessor for SVSine {
    fn process(&mut self, sample_rate: f32, inputs: &[f32]) -> f32 {
        // Get frequency from input or use default
        let frequency = if inputs.len() > 0 && inputs[0] != 0.0 {
            inputs[0]
        } else {
            self.frequency
        };

        let w = 2.0 * PI * frequency / sample_rate;

        let g = if self.w_1 != 0.0 {
            (w / 2.0).tan() / (self.w_1 / 2.0).tan()
        } else {
            1.0
        };

        let cos_w = w.cos();

        let x_next = cos_w * g * self.x + (cos_w - 1.0) * self.output;
        let y_next = (1.0 + cos_w) * g * self.x + cos_w * self.output;

        self.x = x_next;
        self.output = y_next;
        self.w_1 = w;

        self.output
    }
}

// Audio Callback
fn audio_callback(
    data: &mut [f32],
    graph: &mut Graph,
    freq_input: &ValueKey,
    output: &OutputEndpoint,
    rx: &std::sync::mpsc::Receiver<SynthParams>,
    channels: usize,
) {
    let mut latest_params = None;
    while let Ok(params) = rx.try_recv() {
        latest_params = Some(params);
    }

    if let Some(params) = latest_params {
        graph.set_value(*freq_input, params.frequency, 441);
    }

    for frame in data.chunks_mut(channels) {
        graph.process();

        if let Some(value) = graph.get_value(output) {
            for sample in frame.iter_mut() {
                *sample = value;
            }
        }
    }
}

// GUI
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
                    ui.vertical(|ui| {
                        ui.heading("Oscillator");
                        ui.add_space(20.0);

                        // Frequency
                        ui.label("Frequency");
                        if ui
                            .add(
                                egui::Slider::new(&mut self.params.frequency, 20.0..=2000.0)
                                    .step_by(1.0),
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

// Set up threads and build graph
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
        let oscillator = graph.add_node(SVSine::new(100.0, 0.1));

        let low = graph.transform(oscillator.output(), |x| x * 0.1);

        // choose output endpoint
        let output = low;

        // create value input endpoints for the UI
        let freq_input = graph
            .insert_value_input(oscillator.frequency(), 440.0)
            .expect("Failed to insert carrier frequency input");
        // ==========================================================

        let channels = config.channels as usize;

        let stream = device
            .build_output_stream(
                &config.clone().into(),
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    audio_callback(data, &mut graph, &freq_input, &output, &rx, channels);
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
