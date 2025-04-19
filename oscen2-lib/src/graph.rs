use std::ops::Shr;

use arrayvec::ArrayVec;
use hound;
use slotmap::{new_key_type, SecondaryMap, SlotMap};

pub const MAX_EVENTS: usize = 256;
pub const MAX_CONNECTIONS_PER_OUTPUT: usize = 1024;
pub const MAX_DELAY_SAMPLES: usize = 44100;
pub const MAX_NODE_ENDPOINTS: usize = 16;

new_key_type! { pub struct NodeKey; }
new_key_type! { pub struct ValueKey; }

/// Everything the graph needs to know about a node.
pub struct NodeData {
    pub processor: Box<dyn SignalProcessor>,
    pub inputs: ArrayVec<ValueKey, MAX_NODE_ENDPOINTS>,
    pub outputs: ArrayVec<ValueKey, MAX_NODE_ENDPOINTS>,
}

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

#[derive(Copy, Clone, Debug)]
pub struct InputEndpoint {
    // TODO: why does making this pub(crate) cause #[derive(Node)]
    // to fail outside of oscen2 lib?
    pub key: ValueKey,
}

impl InputEndpoint {
    pub fn new(key: ValueKey) -> Self {
        Self { key }
    }

    pub fn key(&self) -> ValueKey {
        self.key
    }
}

#[derive(Copy, Clone, Debug)]
pub struct OutputEndpoint {
    pub key: ValueKey,
}

pub struct Connection {
    from: OutputEndpoint,
    to: InputEndpoint,
}

// Builder for creating multiple connections from a single output
pub struct ConnectionBuilder {
    from: OutputEndpoint,
    connections: ArrayVec<Connection, MAX_CONNECTIONS_PER_OUTPUT>,
}

impl OutputEndpoint {
    pub fn new(key: ValueKey) -> Self {
        Self { key }
    }

    pub fn to(self, input: InputEndpoint) -> ConnectionBuilder {
        // Reuse the Shr operator implementation
        self.shr(input)
    }

    pub fn key(&self) -> ValueKey {
        self.key
    }
}

impl ConnectionBuilder {
    pub fn and(mut self, to: InputEndpoint) -> Self {
        self.connections.push(Connection {
            from: self.from,
            to,
        });
        self
    }
}

impl Shr<InputEndpoint> for OutputEndpoint {
    type Output = ConnectionBuilder;

    fn shr(self, to: InputEndpoint) -> ConnectionBuilder {
        let mut builder = ConnectionBuilder {
            from: self,
            connections: ArrayVec::new(),
        };
        builder.connections.push(Connection { from: self, to });
        builder
    }
}

// Allow ConnectionBuilder to be converted into a Vec<Connection>
impl From<ConnectionBuilder> for ArrayVec<Connection, MAX_CONNECTIONS_PER_OUTPUT> {
    fn from(builder: ConnectionBuilder) -> Self {
        builder.connections
    }
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
    pub nodes: SlotMap<NodeKey, NodeData>,
    pub values: SlotMap<ValueKey, f32>,
    pub connections: SecondaryMap<ValueKey, ArrayVec<ValueKey, MAX_CONNECTIONS_PER_OUTPUT>>,
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
            endpoint_types: SecondaryMap::new(),
            event_queue: ArrayVec::new(),
        }
    }

    pub fn add_node<T: ProcessingNode + 'static>(&mut self, mut node: T) -> T::Endpoints {
        node.init(self.sample_rate);

        let inputs = (0..node.input_endpoints().len())
            .map(|_| self.values.insert(0.0))
            .collect::<ArrayVec<_, MAX_NODE_ENDPOINTS>>();

        let outputs = (0..node.output_endpoints().len())
            .map(|_| self.values.insert(0.0))
            .collect::<ArrayVec<_, MAX_NODE_ENDPOINTS>>();

        let node_key = self.nodes.insert(NodeData {
            processor: Box::new(node),
            inputs: inputs.clone(),
            outputs: outputs.clone(),
        });

        T::create_endpoints(node_key, inputs, outputs)
    }

    pub fn get_input(&self, node: NodeKey, index: usize) -> Option<ValueKey> {
        self.nodes
            .get(node)
            .and_then(|node_data| node_data.inputs.get(index))
            .copied()
    }

    pub fn get_input_by_name(&self, node: NodeKey, name: &str) -> Option<ValueKey> {
        self.nodes.get(node).and_then(|node_data| {
            node_data
                .processor
                .input_index(name)
                .and_then(|idx| node_data.inputs.get(idx))
                .copied()
        })
    }

    pub fn insert_value_input(
        &mut self,
        input: InputEndpoint,
        initial_value: f32,
    ) -> Option<ValueKey> {
        let value = self.values.get_mut(input.key())?;
        *value = initial_value;
        self.endpoint_types
            .insert(input.key(), EndpointType::value(initial_value));
        Some(input.key())
    }

    pub fn set_input_by_name(&mut self, node_key: NodeKey, name: &str, value: f32) {
        if let Some(node_data) = self.nodes.get(node_key) {
            if let Some(index) = node_data.processor.input_index(name) {
                if let Some(value_key) = node_data.inputs.get(index) {
                    self.values[*value_key] = value;
                }
            }
        }
    }

    pub fn get_node_output(&self, node: NodeKey, index: usize) -> Option<ValueKey> {
        self.nodes
            .get(node)
            .and_then(|node_data| node_data.outputs.get(index))
            .copied()
    }

    pub fn connect(&mut self, from: OutputEndpoint, to: InputEndpoint) {
        self.connections
            .entry(from.key())
            .unwrap()
            .or_insert_with(ArrayVec::new)
            .push(to.key());
    }

    pub fn connect_all(&mut self, connections: Vec<ConnectionBuilder>) {
        for builder in connections {
            for Connection { from, to } in builder.connections {
                self.connect(from, to);
            }
        }
    }

    /// Creates a node that applies a function to the output of another node.
    ///
    /// # Arguments
    /// * `from` - The output endpoint to transform
    /// * `f` - The function to apply to the output value
    ///
    /// # Returns
    /// A new output endpoint representing the transformed signal
    pub fn transform(&mut self, from: OutputEndpoint, f: fn(f32) -> f32) -> OutputEndpoint {
        let node = FunctionNode::new(f);
        let processor: Box<dyn SignalProcessor> = Box::new(node); // Explicitly type as Box<dyn SignalProcessor>

        // Create input value key
        let input_key = self.values.insert(0.0);
        let mut input_keys = ArrayVec::new();
        input_keys.push(input_key);

        // Create output value key
        let output_key = self.values.insert(0.0);
        let mut output_keys = ArrayVec::new();
        output_keys.push(output_key);

        // Insert NodeData
        let node_key = self.nodes.insert(NodeData {
            processor,
            inputs: input_keys.clone(),   // Clone for NodeData
            outputs: output_keys.clone(), // Clone for NodeData
        });

        let output = OutputEndpoint { key: output_key };

        self.connect(from, InputEndpoint { key: input_key });

        output
    }

    /// Creates a node that combines two signals using a binary function.
    ///
    /// # Arguments
    /// * `from1` - The first output endpoint to combine
    /// * `from2` - The second output endpoint to combine
    /// * `f` - The binary function to apply to both outputs
    ///
    /// # Returns
    /// A new output endpoint representing the combined signal
    pub fn combine(
        &mut self,
        from1: OutputEndpoint,
        from2: OutputEndpoint,
        f: fn(f32, f32) -> f32,
    ) -> OutputEndpoint {
        let node = BinaryFunctionNode::new(f);
        let processor: Box<dyn SignalProcessor> = Box::new(node); // Explicitly type as Box<dyn SignalProcessor>

        // Create input value keys
        let input_key1 = self.values.insert(0.0);
        let input_key2 = self.values.insert(0.0);
        let mut input_keys = ArrayVec::new();
        input_keys.push(input_key1);
        input_keys.push(input_key2);

        // Create output value key
        let output_key = self.values.insert(0.0);
        let mut output_keys = ArrayVec::new();
        output_keys.push(output_key);

        // Insert NodeData
        let node_key = self.nodes.insert(NodeData {
            processor,
            inputs: input_keys.clone(),   // Clone for NodeData
            outputs: output_keys.clone(), // Clone for NodeData
        });

        let output = OutputEndpoint { key: output_key };

        self.connect(from1, InputEndpoint { key: input_key1 });
        self.connect(from2, InputEndpoint { key: input_key2 });

        output
    }

    pub fn multiply(&mut self, a: OutputEndpoint, b: OutputEndpoint) -> OutputEndpoint {
        self.combine(a, b, |x, y| x * y)
    }

    pub fn add(&mut self, a: OutputEndpoint, b: OutputEndpoint) -> OutputEndpoint {
        self.combine(a, b, |x, y| x + y)
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
        self.values.get(endpoint.key()).copied()
    }

    pub fn send_event(&mut self, input: ValueKey, event: EventData) {
        if let Some(EndpointType::Event) = self.endpoint_types.get(input) {
            self.event_queue.push((input, event));
        }
    }

    /// Process one sample of audio for all nodes in the graph.
    ///
    /// This method:
    /// 1. Updates any parameter values that are currently ramping
    /// 2. Processes each node in the graph in their current order
    /// 3. Propagates output values to connected inputs
    /// 4. Handles any pending events in the event queue
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
            let mut input_values = ArrayVec::<f32, MAX_NODE_ENDPOINTS>::new();

            // Get input values
            for &input_key in &node.inputs {
                input_values.push(self.values[input_key]);
            }

            // Process the node with its inputs to get the output
            let output = node.processor.process(self.sample_rate, &input_values);

            // Store the output value in the first output of the node
            if let Some(&output_key) = node.outputs.get(0) {
                self.values[output_key] = output;

                // Propagate the output to all connected inputs
                if let Some(connections) = self.connections.get(output_key) {
                    for &target_input in connections {
                        self.values[target_input] = output;
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
            // Find the output key of the last node's first output
            // Note: This assumes the last node added is the final output node.
            // This might need refinement based on your desired graph structure.
            if let Some(output_key) = self
                .nodes
                .values()
                .last()
                .and_then(|node_data| node_data.outputs.get(0))
                .copied()
            {
                if let Some(&value) = self.values.get(output_key) {
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

// The ProcessingNode trait is automatically implemented by the Node macro.
// This trait provides the necessary functionality to create node endpoints
// and integrate custom node types into the audio graph. When you use the
// #[derive(Node)] macro, it generates all the boilerplate code needed to
// implement this trait, including the creation of strongly-typed endpoint
// accessors for inputs and outputs.
pub trait ProcessingNode: SignalProcessor + EndpointDefinition {
    type Endpoints;

    fn create_endpoints(
        node_key: NodeKey,
        inputs: ArrayVec<ValueKey, MAX_NODE_ENDPOINTS>,
        outputs: ArrayVec<ValueKey, MAX_NODE_ENDPOINTS>,
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
        _inputs: ArrayVec<ValueKey, MAX_NODE_ENDPOINTS>,
        _outputs: ArrayVec<ValueKey, MAX_NODE_ENDPOINTS>,
    ) -> Self::Endpoints {
        node_key
    }
}

#[derive(Debug)]
struct BinaryFunctionNode {
    f: fn(f32, f32) -> f32,
}

impl BinaryFunctionNode {
    fn new(f: fn(f32, f32) -> f32) -> Self {
        Self { f }
    }
}

impl EndpointDefinition for BinaryFunctionNode {
    fn input_endpoints(&self) -> Vec<EndpointMetadata> {
        vec![
            EndpointMetadata {
                name: "input1",
                index: 0,
            },
            EndpointMetadata {
                name: "input2",
                index: 1,
            },
        ]
    }

    fn output_endpoints(&self) -> Vec<EndpointMetadata> {
        vec![EndpointMetadata {
            name: "output",
            index: 0,
        }]
    }
}

impl SignalProcessor for BinaryFunctionNode {
    fn process(&mut self, _sample_rate: f32, inputs: &[f32]) -> f32 {
        (self.f)(inputs[0], inputs[1])
    }
}

impl ProcessingNode for BinaryFunctionNode {
    type Endpoints = NodeKey;

    fn create_endpoints(
        node_key: NodeKey,
        _inputs: ArrayVec<ValueKey, MAX_NODE_ENDPOINTS>,
        _outputs: ArrayVec<ValueKey, MAX_NODE_ENDPOINTS>,
    ) -> Self::Endpoints {
        node_key
    }
}

/// Represents the mode of operation for a buffer's size management
/// - PowerOfTwo: Buffer size is rounded up to the next power of 2
/// - Exact: Buffer size is kept exactly as specified
#[derive(Clone, Copy, Debug)]
pub enum BufferMode {
    PowerOfTwo,
    Exact,
}

impl Default for BufferMode {
    fn default() -> Self {
        BufferMode::PowerOfTwo
    }
}

/// A ring buffer implementation with linear and cubic interpolation for reading values.
#[derive(Clone, Debug)]
pub struct RingBuffer {
    /// The internal buffer storing samples
    buffer: ArrayVec<f32, MAX_DELAY_SAMPLES>,
    /// Current write position in the buffer
    write_pos: usize,
    /// Mask used for efficient wrapping of indices (must be power of 2 minus 1)
    size: usize,
    /// Buffer mode: PowerOfTwo for power-of-2 sized buffers, Exact for exact sizes
    mode: BufferMode,
}

impl RingBuffer {
    /// Creates a new RingBuffer with the specified size.
    pub fn new(size: usize) -> Self {
        Self::with_mode(size, BufferMode::default())
    }

    /// Creates a new RingBuffer with the specified size and mode
    /// The buffer size can either be the exact specified size, or the next power of two.
    pub fn with_mode(size: usize, mode: BufferMode) -> Self {
        let clamped_size = size.min(MAX_DELAY_SAMPLES);
        let buffer_size = match mode {
            BufferMode::PowerOfTwo => clamped_size.next_power_of_two(),
            BufferMode::Exact => clamped_size,
        };

        let buffer: ArrayVec<f32, MAX_DELAY_SAMPLES> = ArrayVec::new();

        Self {
            buffer,
            write_pos: 0,
            size: buffer_size - 1,
            mode,
        }
    }

    /// Pushes a new value into the buffer, advancing the write position
    pub fn push(&mut self, v: f32) {
        self.write_pos = (self.write_pos + 1) & self.size;
        self.buffer[self.write_pos] = v;
    }

    /// Calculates the read position for a given offset, handling wrapping
    fn read_pos(&self, offset: f32) -> f32 {
        let n = (self.size + 1) as f32;
        let mut rp = self.write_pos as f32 - offset;
        // Wrap the read position to stay within buffer bounds
        while rp >= n {
            rp -= n;
        }
        while rp < 0.0 {
            rp += n;
        }
        rp
    }

    /// Gets a value from the buffer using linear interpolation
    fn get_linear(&self, offset: f32) -> f32 {
        let rp = self.read_pos(offset);
        let i = rp.floor() as usize;
        let f = rp - rp.floor();

        let a = self.buffer[i];
        let b = self.buffer[(i + 1) & self.size];

        // Linear interpolation between adjacent samples
        a + f * (b - a)
    }

    /// Gets a value from the buffer using cubic interpolation
    fn get_cubic(&self, offset: f32) -> f32 {
        let rp = self.read_pos(offset);
        let i = rp.floor() as usize;
        let f = rp - rp.floor();

        // Get indices for 4 points needed for cubic interpolation
        let im1 = (i - 1) & self.size;
        let i1 = (i + 1) & self.size;
        let i2 = (i + 2) & self.size;

        let v0 = self.buffer[im1];
        let v1 = self.buffer[i];
        let v2 = self.buffer[i1];
        let v3 = self.buffer[i2];

        // Cubic interpolation coefficients
        let a1 = 0.5 * (v2 - v0);
        let a2 = v0 - 2.5 * v1 + 2.0 * v2 - 0.5 * v3;
        let a3 = 0.5 * (v3 - v0) + 1.5 * (v1 - v2);

        // Cubic interpolation formula
        v1 + f * (a1 + f * (a2 + f * a3))
    }

    /// Gets a value from the buffer using the most appropriate interpolation method
    /// based on the offset distance
    pub fn get(&self, offset: f32) -> f32 {
        let offset = offset.min((self.size + 1) as f32);

        // Use different interpolation methods based on offset:
        // - Cubic interpolation for small offsets
        // - Linear interpolation for larger offsets
        if offset <= 1.0 {
            self.buffer[self.write_pos]
        } else if offset < 4.0 {
            self.get_cubic(offset)
        } else {
            self.get_linear(offset)
        }
    }

    /// Sets the size of the buffer, clamped to the actual buffer length.
    /// If in PowerOfTwo mode, rounds up to next power of 2.
    /// If in Exact mode, uses the clamped size directly.
    pub fn set_size(&mut self, new_size: usize) {
        let clamped_size = new_size.min(self.buffer.len());
        self.size = match self.mode {
            BufferMode::PowerOfTwo => clamped_size.next_power_of_two(),
            BufferMode::Exact => clamped_size,
        };
    }
}
