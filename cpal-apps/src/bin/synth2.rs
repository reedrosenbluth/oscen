use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use iced::{
    widget::{column, slider, text, Container},
    Alignment, Application, Command, Element, Settings, Theme,
};
use oscen2::{EndpointType, Graph, Oscillator, OutputEndpoint, TPT_Filter, ValueKey};
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
            modulator_frequency: 0.5,
            cutoff_frequency: 3000.0,
            q_factor: 0.707,
        }
    }
}

fn audio_callback(
    data: &mut [f32],
    graph: &mut Graph,
    carrier_freq_input: &ValueKey,
    modulator_freq_input: &ValueKey,
    cutoff_freq_input: &ValueKey,
    q_input: &ValueKey,
    output: &OutputEndpoint,
    rx: &std::sync::mpsc::Receiver<SynthParams>,
    channels: usize,
) {
    if let Ok(params) = rx.try_recv() {
        graph.set_value(*carrier_freq_input, params.carrier_frequency, 1000);
        graph.set_value(*modulator_freq_input, params.modulator_frequency, 1000);
        graph.set_value(*cutoff_freq_input, params.cutoff_frequency, 100);
        graph.set_value(*q_input, params.q_factor, 100);
    }

    for frame in data.chunks_mut(channels) {
        graph.process();

        if let Some(value) = graph.get_value(output) {
            // println!("Output value: {}", value);
            for sample in frame.iter_mut() {
                *sample = value;
            }
        }
    }
}

fn main() -> iced::Result {
    let (tx, rx) = channel();

    thread::spawn(move || {
        let host = cpal::default_host();
        let device = host.default_output_device().expect("no output device");
        let config = device.default_output_config().unwrap();
        let sample_rate = config.sample_rate().0 as f32;

        let mut graph = Graph::new(sample_rate);

        let modulator = graph.add_node(Oscillator::sine(0.5, 0.5));
        let carrier = graph.add_node(Oscillator::saw(440.0, 1.0));
        let filter = graph.add_node(TPT_Filter::new(3000.0, 0.707));
        graph.connect(modulator.output(), carrier.frequency_mod());
        graph.connect(carrier.output(), filter.input());

        let output = graph.transform(filter.output(), |x| x.tanh() * 0.5);

        // TODO: don't love the input names as strings
        let carrier_freq_input = graph
            .get_input_by_name(carrier.node_key(), "frequency")
            .expect("Oscillator should have frequency input");
        let modulator_freq_input = graph
            .get_input_by_name(modulator.node_key(), "frequency")
            .expect("Oscillator should have frequency input");
        let cutoff_freq_input = graph
            .get_input_by_name(filter.node_key(), "cutoff")
            .expect("LPF should have cutoff input");
        let q_input = graph
            .get_input_by_name(filter.node_key(), "q")
            .expect("LPF should have Q input");

        // Set up the frequency inputs as a Value endpoints
        graph
            .endpoint_types
            .insert(carrier_freq_input, EndpointType::value(440.0));
        graph
            .endpoint_types
            .insert(modulator_freq_input, EndpointType::value(0.5));
        graph
            .endpoint_types
            .insert(cutoff_freq_input, EndpointType::value(3000.0));
        graph
            .endpoint_types
            .insert(q_input, EndpointType::value(0.707));

        let channels = config.channels() as usize;

        let stream = device
            .build_output_stream(
                &config.clone().into(),
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    audio_callback(
                        data,
                        &mut graph,
                        &carrier_freq_input,
                        &modulator_freq_input,
                        &cutoff_freq_input,
                        &q_input,
                        &output,
                        &rx,
                        channels,
                    );
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

    let mut settings = Settings::with_flags(tx);
    settings.window.size = (300, 400);
    Model::run(settings)
}

struct Model {
    params: SynthParams,
    tx: Sender<SynthParams>,
}

#[derive(Debug, Clone)]
enum Message {
    SetCarrierFrequency(f32),
    SetModulatorFrequency(f32),
    SetCutoffFrequency(f32),
    SetQFactor(f32),
}

impl Application for Model {
    type Message = Message;
    type Theme = Theme;
    type Executor = iced::executor::Default;
    type Flags = Sender<SynthParams>;

    fn new(flags: Sender<SynthParams>) -> (Model, Command<Message>) {
        (
            Model {
                params: SynthParams::default(),
                tx: flags,
            },
            Command::none(),
        )
    }

    fn title(&self) -> String {
        String::from("Oscillator Control")
    }

    fn update(&mut self, message: Message) -> Command<Message> {
        match message {
            Message::SetCarrierFrequency(new_freq) => {
                self.params.carrier_frequency = new_freq;
            }
            Message::SetModulatorFrequency(new_freq) => {
                self.params.modulator_frequency = new_freq;
            }
            Message::SetCutoffFrequency(new_freq) => {
                self.params.cutoff_frequency = new_freq;
            }
            Message::SetQFactor(new_q) => {
                self.params.q_factor = new_q;
            }
        }
        let _ = self.tx.send(self.params);
        Command::none()
    }

    fn view(&self) -> Element<Message> {
        Container::new(
            column![
                text(format!(
                    "Carrier Frequency: {:.1} Hz",
                    self.params.carrier_frequency
                ))
                .size(20),
                slider(
                    20.0..=2000.0,
                    self.params.carrier_frequency,
                    Message::SetCarrierFrequency
                )
                .step(1.0),
                text(format!(
                    "Modulator Frequency: {:.1} Hz",
                    self.params.modulator_frequency
                ))
                .size(20),
                slider(
                    0.1..=40.0,
                    self.params.modulator_frequency,
                    Message::SetModulatorFrequency
                )
                .step(0.1),
                text(format!(
                    "Filter Cutoff: {:.1} Hz",
                    self.params.cutoff_frequency
                ))
                .size(20),
                slider(
                    0.0..=1.0,
                    linear_to_log(self.params.cutoff_frequency, 20.0, 20000.0),
                    |norm| Message::SetCutoffFrequency(log_to_linear(norm, 20.0, 20000.0))
                )
                .step(0.001),
                text(format!("Filter Q: {:.1} Hz", self.params.q_factor)).size(20),
                slider(0.1..=10.0, self.params.q_factor, Message::SetQFactor).step(0.1),
            ]
            .spacing(20)
            .align_items(Alignment::Center),
        )
        .padding(20)
        .center_x()
        .center_y()
        .into()
    }
}

fn linear_to_log(value: f32, min: f32, max: f32) -> f32 {
    (value.ln() - min.ln()) / (max.ln() - min.ln())
}

fn log_to_linear(value: f32, min: f32, max: f32) -> f32 {
    (value * (max.ln() - min.ln()) + min.ln()).exp()
}
