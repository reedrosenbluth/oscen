use oscen2_macros::Node;

use arrayvec::ArrayVec;
use hound;
use slotmap::{new_key_type, SecondaryMap, SlotMap};
use std::f32::consts::PI;

pub const MAX_MODULES: usize = 1024;

new_key_type! { pub struct NodeKey; }
new_key_type! { pub struct ValueKey; }

pub struct InputEndpoint {
    pub(crate) key: ValueKey,
}

pub struct OutputEndpoint {
    pub(crate) key: ValueKey,
}

#[derive(Debug, Default)]
pub struct EndpointMetadata {
    pub name: &'static str,
    pub index: usize,
}

pub trait EndpointDefinition {
    fn input_endpoints(&self) -> Vec<EndpointMetadata>;
    fn output_endpoints(&self) -> Vec<EndpointMetadata>;

    fn input_index(&self, name: &str) -> Option<usize> {
        self.input_endpoints()
            .iter()
            .find(|endpoint| endpoint.name == name)
            .map(|endpoint| endpoint.index)
    }
}

pub trait SignalProcessor: EndpointDefinition {
    fn process(&mut self, sample_rate: f32, inputs: &[f32]) -> f32;
}

// This trait will be implemented by the macro
pub trait ProcessingNode: SignalProcessor + EndpointDefinition {
    type Endpoints;

    fn create_endpoints(
        node_key: NodeKey,
        inputs: ArrayVec<ValueKey, 16>,
        outputs: ArrayVec<ValueKey, 16>,
    ) -> Self::Endpoints;
}

//TODO: replace ArrayVecs with SlotMaps?
pub struct Graph {
    pub sample_rate: f32,
    pub nodes: SlotMap<NodeKey, Box<dyn SignalProcessor>>,
    pub values: SlotMap<ValueKey, f32>,
    pub connections: SecondaryMap<ValueKey, ArrayVec<ValueKey, MAX_MODULES>>,
    pub node_inputs: SecondaryMap<NodeKey, ArrayVec<ValueKey, 16>>,
    pub node_outputs: SecondaryMap<NodeKey, ArrayVec<ValueKey, 16>>,
}

impl Graph {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate,
            nodes: SlotMap::with_key(),
            values: SlotMap::with_key(),
            connections: SecondaryMap::new(),
            node_inputs: SecondaryMap::new(),
            node_outputs: SecondaryMap::new(),
        }
    }

    pub fn add_node<T: ProcessingNode + 'static>(&mut self, node: T) -> T::Endpoints {
        let input_count = node.input_endpoints().len();
        let output_count = node.output_endpoints().len();

        let node_key = self.nodes.insert(Box::new(node));

        let mut input_keys = ArrayVec::new();
        for _ in 0..input_count {
            let value_key = self.values.insert(0.0);
            input_keys.push(value_key);
        }
        self.node_inputs.insert(node_key, input_keys.clone());

        let mut output_keys = ArrayVec::new();
        for _ in 0..output_count {
            let value_key = self.values.insert(0.0);
            output_keys.push(value_key);
        }
        self.node_outputs.insert(node_key, output_keys.clone());

        T::create_endpoints(node_key, input_keys, output_keys)
    }

    pub fn get_input(&self, node: NodeKey, index: usize) -> Option<ValueKey> {
        self.node_inputs
            .get(node)
            .and_then(|inputs| inputs.get(index))
            .copied()
    }

    pub fn get_input_by_name(&self, node: NodeKey, name: &str) -> Option<ValueKey> {
        self.nodes
            .get(node)
            .and_then(|node| node.input_index(name))
            .and_then(|idx| self.get_input(node, idx))
    }

    pub fn get_output(&self, node: NodeKey, index: usize) -> Option<ValueKey> {
        self.node_outputs
            .get(node)
            .and_then(|outputs| outputs.get(index))
            .copied()
    }

    pub fn connect(&mut self, from: OutputEndpoint, to: InputEndpoint) {
        self.connections
            .entry(from.key)
            .unwrap()
            .or_insert_with(ArrayVec::new)
            .push(to.key);
    }

    /// Process one sample of audio for all nodes in the graph
    pub fn process(&mut self) {
        // Iterate through all nodes in the graph
        for (node_key, node) in self.nodes.iter_mut() {
            // Create array to store input values for this node
            let mut input_values = ArrayVec::<f32, 16>::new();

            // Get the input keys for this node
            if let Some(input_keys) = self.node_inputs.get(node_key) {
                // For each input key, find its connected output value
                for &input_key in input_keys {
                    let mut input_value = 0.0;

                    // Search through all connections to find if this input is connected
                    for (output_key, connections) in self.connections.iter() {
                        if connections.contains(&input_key) {
                            // Found a connection - get the output value
                            input_value = self.values[output_key];
                            break;
                        }
                    }

                    // Store the input value (0.0 if no connection found)
                    input_values.push(input_value);
                }
            }

            // Process the node with its inputs to get the output
            let output = node.process(self.sample_rate, &input_values);

            // Store the output value in the first output of the node
            if let Some(output_keys) = self.node_outputs.get(node_key) {
                if let Some(&output_key) = output_keys.get(0) {
                    self.values[output_key] = output;
                }
            }
        }
    }

    pub fn render_to_file(
        &mut self,
        duration_secs: f32,
        path: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let spec = hound::WavSpec {
            channels: 2, // Stereo output
            sample_rate: self.sample_rate as u32,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        };

        let mut writer = hound::WavWriter::create(path, spec)?;
        let num_samples = (duration_secs * self.sample_rate) as u32;

        for _ in 0..num_samples {
            self.process();
            if let Some(output_key) = self.node_outputs.values().last().and_then(|v| v.get(0)) {
                if let Some(&value) = self.values.get(*output_key) {
                    // Write same value to both channels for now
                    writer.write_sample(value)?; // Left channel
                    writer.write_sample(value)?; // Right channel
                }
            }
        }

        writer.finalize()?;
        Ok(())
    }
}

#[derive(Node)]
pub struct Oscillator {
    #[input]
    phase: f32,
    #[input]
    frequency: f32,
    #[input]
    amplitude: f32,

    #[output]
    signal: f32,

    waveform: fn(f32) -> f32,
}

impl Oscillator {
    pub fn new(frequency: f32, amplitude: f32, waveform: fn(f32) -> f32) -> Self {
        Self {
            phase: 0.0,
            frequency,
            amplitude,
            waveform,
            signal: 0.0,
        }
    }

    pub fn sine(frequency: f32, amplitude: f32) -> Self {
        Self::new(frequency, amplitude, |p| (p * 2.0 * PI).sin())
    }

    pub fn square(frequency: f32, amplitude: f32) -> Self {
        Self::new(frequency, amplitude, |p| if p < 0.5 { 1.0 } else { -1.0 })
    }
}

impl SignalProcessor for Oscillator {
    fn process(&mut self, sample_rate: f32, inputs: &[f32]) -> f32 {
        let phase_mod = self.get_phase(inputs);
        let freq_mod = self.get_frequency(inputs);
        let amp_mod = self.get_amplitude(inputs);

        let freq = self.frequency + (freq_mod * 100.0);
        let amplitude = self.amplitude * (1.0 + amp_mod);

        self.signal = (self.waveform)(self.phase) * amplitude;

        self.phase += freq / sample_rate;
        self.phase %= 1.0; // Keep phase between 0 and 1

        self.signal
    }
}

#[test]
fn test_audio_render_fm() {
    let mut graph = Graph::new(44100.0);

    let modulator = graph.add_node(Oscillator::sine(880.0, 0.5));
    let carrier = graph.add_node(Oscillator::sine(254.37, 0.5));

    graph.connect(modulator.signal(), carrier.frequency());

    graph
        .render_to_file(5.0, "test_output_fm.wav")
        .expect("Failed to render audio");
}

#[test]
fn test_audio_render_fm2() {
    let mut graph = Graph::new(44100.0);

    let modulator = graph.add_node(Oscillator::sine(0.5, 0.5));
    let carrier = graph.add_node(Oscillator::sine(440., 0.5));

    graph.connect(modulator.signal(), carrier.frequency());

    graph
        .render_to_file(5.0, "test_output_fm2.wav")
        .expect("Failed to render audio");
}

#[test]
fn test_audio_render_am() {
    let mut graph = Graph::new(44100.0);

    let lfo = graph.add_node(Oscillator::sine(0.5, 0.5));
    let osc2 = graph.add_node(Oscillator::sine(440., 0.5));

    graph.connect(lfo.signal(), osc2.amplitude());

    graph
        .render_to_file(5.0, "test_output_am.wav") // 4 seconds to hear 2 full cycles
        .expect("Failed to render audio");
}

#[test]
fn test_audio_render_debug() {
    let mut graph = Graph::new(44100.0);

    let modulator = graph.add_node(Oscillator::sine(5.0, 100.0));
    let carrier = graph.add_node(Oscillator::sine(440.0, 0.5));

    graph.connect(modulator.signal(), carrier.frequency());

    // Process just 10 samples
    for i in 0..10 {
        println!("\nProcessing sample {}", i);
        graph.process();
    }
}
