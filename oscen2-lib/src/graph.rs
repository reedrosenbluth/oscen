use arrayvec::ArrayVec;
use hound;
use slotmap::{new_key_type, SecondaryMap, SlotMap};

pub const MAX_EVENTS: usize = 256;
pub const MAX_CONNECTIONS_PER_OUTPUT: usize = 1024;

new_key_type! { pub struct NodeKey; }
new_key_type! { pub struct ValueKey; }

#[derive(Debug)]
pub enum EndpointType {
    Stream(ValueKey),
    Value {
        current: f32,
        target: f32,
        ramp_samples_remaining: u32,
        ramp_total_samples: u32,
    },
    Event,
}

impl EndpointType {
    pub fn value(initial_value: f32) -> Self {
        Self::Value {
            current: initial_value,
            target: initial_value,
            ramp_samples_remaining: 0,
            ramp_total_samples: 0,
        }
    }
}

#[derive(Debug)]
pub enum EventData {
    Float(f32),
    Int(i32),
    Trigger,
}

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

//TODO: replace ArrayVecs with SlotMaps?
pub struct Graph {
    pub sample_rate: f32,
    pub nodes: SlotMap<NodeKey, Box<dyn SignalProcessor>>,
    pub values: SlotMap<ValueKey, f32>,
    pub connections: SecondaryMap<ValueKey, ArrayVec<ValueKey, MAX_CONNECTIONS_PER_OUTPUT>>,
    pub node_inputs: SecondaryMap<NodeKey, ArrayVec<ValueKey, 16>>,
    pub node_outputs: SecondaryMap<NodeKey, ArrayVec<ValueKey, 16>>,
    pub endpoint_types: SecondaryMap<ValueKey, EndpointType>,
    pub event_queue: ArrayVec<(ValueKey, EventData), MAX_EVENTS>,
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
            endpoint_types: SecondaryMap::new(),
            event_queue: ArrayVec::new(),
        }
    }

    pub fn add_node<T: ProcessingNode + 'static>(&mut self, mut node: T) -> T::Endpoints {
        node.init(self.sample_rate);

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

    pub fn set_input_by_name(&mut self, node_key: NodeKey, name: &str, value: f32) {
        if let Some(node) = self.nodes.get(node_key) {
            if let Some(index) = node.input_index(name) {
                if let Some(inputs) = self.node_inputs.get(node_key) {
                    if let Some(value_key) = inputs.get(index) {
                        self.values[*value_key] = value;
                    }
                }
            }
        }
    }

    pub fn get_node_output(&self, node: NodeKey, index: usize) -> Option<ValueKey> {
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

    pub fn transform(&mut self, from: OutputEndpoint, f: fn(f32) -> f32) -> OutputEndpoint {
        let node = FunctionNode::new(f);
        let node_key = self.nodes.insert(Box::new(node));

        // Create input value key
        let input_key = self.values.insert(0.0);
        let mut input_keys = ArrayVec::new();
        input_keys.push(input_key);
        self.node_inputs.insert(node_key, input_keys);

        // Create output value key
        let output_key = self.values.insert(0.0);
        let mut output_keys = ArrayVec::new();
        output_keys.push(output_key);
        self.node_outputs.insert(node_key, output_keys);

        let output = OutputEndpoint { key: output_key };

        self.connect(from, InputEndpoint { key: input_key });

        output
    }

    pub fn set_value(&mut self, input: ValueKey, value: f32, ramp_samples: u32) {
        if let Some(EndpointType::Value {
            target,
            ramp_samples_remaining,
            ramp_total_samples,
            ..
        }) = self.endpoint_types.get_mut(input)
        {
            *target = value;
            *ramp_samples_remaining = ramp_samples;
            *ramp_total_samples = ramp_samples;
        }
    }

    pub fn get_value(&self, endpoint: &OutputEndpoint) -> Option<f32> {
        self.values.get(endpoint.key).copied()
    }

    pub fn send_event(&mut self, input: ValueKey, event: EventData) {
        if let Some(EndpointType::Event) = self.endpoint_types.get(input) {
            self.event_queue.push((input, event));
        }
    }

    /// Process one sample of audio for all nodes in the graph
    pub fn process(&mut self) {
        // Process value ramping
        for (value_key, endpoint_type) in self.endpoint_types.iter_mut() {
            if let EndpointType::Value {
                current,
                target,
                ramp_samples_remaining,
                ramp_total_samples,
            } = endpoint_type
            {
                // if *ramp_samples_remaining > 0 {
                //     let progress = (*ramp_total_samples - *ramp_samples_remaining) as f32
                //         / *ramp_total_samples as f32;
                //     *current = *current + (*target - *current) * progress;
                //     *ramp_samples_remaining -= 1;
                // }
                if *ramp_samples_remaining > 0 {
                    let increment = (*target - *current) / (*ramp_total_samples as f32);
                    *current += increment;
                    *ramp_samples_remaining -= 1;
                }
                // Update the actual input value with the current value
                self.values[value_key] = *current;
            }
        }

        // Iterate through all nodes in the graph
        for (node_key, node) in self.nodes.iter_mut() {
            //TODO: should this be 16?
            let mut input_values = ArrayVec::<f32, 16>::new();

            // Get input values
            if let Some(input_keys) = self.node_inputs.get(node_key) {
                for &input_key in input_keys {
                    input_values.push(self.values[input_key]);
                }
            }

            // Process the node with its inputs to get the output
            let output = node.process(self.sample_rate, &input_values);

            // Store the output value in the first output of the node
            if let Some(output_keys) = self.node_outputs.get(node_key) {
                if let Some(&output_key) = output_keys.get(0) {
                    self.values[output_key] = output;

                    // Propagate the output to all connected inputs
                    if let Some(connections) = self.connections.get(output_key) {
                        for &target_input in connections {
                            self.values[target_input] = output;
                        }
                    }
                }
            }

            // Process events
            while let Some((target_input, event)) = self.event_queue.pop() {
                // TODO: Handle event processing
                // Maybe add event handlers to the SignalProcessor trait?
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

pub trait SignalProcessor: EndpointDefinition + Send + std::fmt::Debug {
    fn init(&mut self, _sample_rate: f32) {}
    fn process(&mut self, sample_rate: f32, inputs: &[f32]) -> f32;
}

// This trait will be implemented by the Node macro
pub trait ProcessingNode: SignalProcessor + EndpointDefinition {
    type Endpoints;

    fn create_endpoints(
        node_key: NodeKey,
        inputs: ArrayVec<ValueKey, 16>,
        outputs: ArrayVec<ValueKey, 16>,
    ) -> Self::Endpoints;
}

#[derive(Debug)]
struct FunctionNode {
    f: fn(f32) -> f32,
}

impl FunctionNode {
    fn new(f: fn(f32) -> f32) -> Self {
        Self { f }
    }
}

impl EndpointDefinition for FunctionNode {
    fn input_endpoints(&self) -> Vec<EndpointMetadata> {
        vec![EndpointMetadata {
            name: "input",
            index: 0,
        }]
    }

    fn output_endpoints(&self) -> Vec<EndpointMetadata> {
        vec![EndpointMetadata {
            name: "output",
            index: 0,
        }]
    }
}

impl SignalProcessor for FunctionNode {
    fn process(&mut self, _sample_rate: f32, inputs: &[f32]) -> f32 {
        (self.f)(inputs[0])
    }
}

impl ProcessingNode for FunctionNode {
    type Endpoints = NodeKey;

    fn create_endpoints(
        node_key: NodeKey,
        _inputs: ArrayVec<ValueKey, 16>,
        _outputs: ArrayVec<ValueKey, 16>,
    ) -> Self::Endpoints {
        node_key
    }
}
