use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::fmt;
use std::ops::Shr;

use arrayvec::ArrayVec;
use hound;
use slotmap::{new_key_type, SecondaryMap, SlotMap};

pub const MAX_EVENTS: usize = 256;
pub const MAX_CONNECTIONS_PER_OUTPUT: usize = 1024;
pub const MAX_NODE_ENDPOINTS: usize = 16;

new_key_type! { pub struct NodeKey; }
new_key_type! { pub struct ValueKey; }

#[derive(Debug, Clone)]
pub enum GraphError {
    CycleDetected(Vec<NodeKey>),
}

impl fmt::Display for GraphError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GraphError::CycleDetected(nodes) => {
                write!(f, "Invalid cycle detected in graph. Cycles must contain at least one Delay node. Cycle contains {} nodes", nodes.len())
            }
        }
    }
}

impl Error for GraphError {}

/// Everything the graph needs to know about a node.
pub struct NodeData {
    pub processor: Box<dyn SignalProcessor>,
    pub inputs: ArrayVec<ValueKey, MAX_NODE_ENDPOINTS>,
    pub outputs: ArrayVec<ValueKey, MAX_NODE_ENDPOINTS>,
}

#[derive(Debug)]
pub enum EndpointType {
    Stream(ValueKey),
    Value,
    Event,
}

#[derive(Debug)]
pub enum EventData {
    Float(f32),
    Int(i32),
    Trigger,
}

#[repr(transparent)]
#[derive(Copy, Clone, Debug)]
pub struct InputEndpoint {
    key: ValueKey,
}

impl InputEndpoint {
    pub fn new(key: ValueKey) -> Self {
        Self { key }
    }

    pub fn key(&self) -> ValueKey {
        self.key
    }
}

#[repr(transparent)]
#[derive(Copy, Clone, Debug)]
pub struct OutputEndpoint {
    key: ValueKey,
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
    fn input_endpoints(&self) -> &'static [EndpointMetadata];
    fn output_endpoints(&self) -> &'static [EndpointMetadata];

    fn input_index(&self, name: &str) -> Option<usize> {
        self.input_endpoints()
            .iter()
            .find(|endpoint| endpoint.name == name)
            .map(|endpoint| endpoint.index)
    }

    fn output_index(&self, name: &str) -> Option<usize> {
        self.output_endpoints()
            .iter()
            .find(|endpoint| endpoint.name == name)
            .map(|endpoint| endpoint.index)
    }
}

use crate::Node;

/// A built-in node for accepting external audio input into the graph.
/// This node provides a simple pass-through from a value input to an audio output,
/// eliminating the need for users to implement their own audio input nodes.
#[derive(Debug, Node)]
pub struct AudioInput {
    #[input]
    input_value: f32,

    #[output]
    output: f32,
}

impl AudioInput {
    pub fn new() -> Self {
        Self {
            input_value: 0.0,
            output: 0.0,
        }
    }
}

impl Default for AudioInput {
    fn default() -> Self {
        Self::new()
    }
}

impl SignalProcessor for AudioInput {
    fn process(&mut self, _sample_rate: f32, inputs: &[f32]) -> f32 {
        // The input_value is at index 0 of the inputs array
        let input_val = if !inputs.is_empty() { inputs[0] } else { 0.0 };
        self.output = input_val;
        self.output
    }
}

pub struct Graph {
    pub sample_rate: f32,
    pub nodes: SlotMap<NodeKey, NodeData>,
    pub values: SlotMap<ValueKey, f32>,
    pub connections: SecondaryMap<ValueKey, ArrayVec<ValueKey, MAX_CONNECTIONS_PER_OUTPUT>>,
    pub endpoint_types: SecondaryMap<ValueKey, EndpointType>,
    // TODO: reconsider this
    pub event_queue: ArrayVec<(ValueKey, EventData), MAX_EVENTS>,

    // Topology tracking for sorted processing order
    node_order: Vec<NodeKey>,
    topology_dirty: bool,

    // Performance optimization: cache which node owns each value
    value_to_node: SecondaryMap<ValueKey, NodeKey>,

    // Active ramps: endpoints with ongoing ramps are updated per-sample
    active_ramps: Vec<ActiveRamp>,
    ramp_indices: SecondaryMap<ValueKey, usize>,
}

#[derive(Copy, Clone, Debug)]
struct ActiveRamp {
    key: ValueKey,
    step: f32,
    remaining: u32,
    target: f32,
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
            node_order: Vec::new(),
            topology_dirty: true,
            value_to_node: SecondaryMap::new(),
            active_ramps: Vec::new(),
            ramp_indices: SecondaryMap::new(),
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

        // Cache which node owns these values for fast lookup
        for &value_key in &inputs {
            self.value_to_node.insert(value_key, node_key);
        }
        for &value_key in &outputs {
            self.value_to_node.insert(value_key, node_key);
        }

        // Mark topology as needing re-sort
        self.topology_dirty = true;

        T::create_endpoints(node_key, inputs, outputs)
    }

    /// Adds a built-in audio input node to the graph.
    /// This provides a convenient way to accept external audio input without
    /// having to implement a custom audio input node.
    ///
    /// Returns a tuple of:
    /// - The audio input node endpoints (for connecting the output)
    /// - The ValueKey for setting input samples via set_value()
    ///
    /// Example:
    /// ```
    /// let (input_node, input_key) = graph.add_audio_input();
    /// graph.connect(input_node.output(), filter.input());
    /// // Later in process loop:
    /// graph.set_value(input_key, audio_sample);
    /// ```
    pub fn add_audio_input(&mut self) -> (<AudioInput as ProcessingNode>::Endpoints, ValueKey) {
        let input_node = self.add_node(AudioInput::new());
        let input_key = self
            .insert_value_input(input_node.input_value(), 0.0)
            .expect("Failed to insert audio input value");
        (input_node, input_key)
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
        let key = input.key();
        let value = self.values.get_mut(key)?;
        *value = initial_value;
        self.endpoint_types.insert(key, EndpointType::Value);
        // Ensure there's no stale ramp for this key
        self.remove_active_ramp(key);
        Some(key)
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
            .or_default()
            .push(to.key());

        // Mark topology as needing re-sort
        self.topology_dirty = true;
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

        // Cache which node owns these values
        for &value_key in &input_keys {
            self.value_to_node.insert(value_key, node_key);
        }
        for &value_key in &output_keys {
            self.value_to_node.insert(value_key, node_key);
        }

        // Mark topology as needing re-sort
        self.topology_dirty = true;

        let output = OutputEndpoint::new(output_key);

        self.connect(from, InputEndpoint::new(input_key));

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

        // Cache which node owns these values
        for &value_key in &input_keys {
            self.value_to_node.insert(value_key, node_key);
        }
        for &value_key in &output_keys {
            self.value_to_node.insert(value_key, node_key);
        }

        // Mark topology as needing re-sort
        self.topology_dirty = true;

        let output = OutputEndpoint::new(output_key);

        self.connect(from1, InputEndpoint::new(input_key1));
        self.connect(from2, InputEndpoint::new(input_key2));

        output
    }

    pub fn multiply(&mut self, a: OutputEndpoint, b: OutputEndpoint) -> OutputEndpoint {
        self.combine(a, b, |x, y| x * y)
    }

    pub fn add(&mut self, a: OutputEndpoint, b: OutputEndpoint) -> OutputEndpoint {
        self.combine(a, b, |x, y| x + y)
    }

    /// Sets a value immediately without ramping.
    pub fn set_value(&mut self, input: ValueKey, value: f32) {
        if matches!(self.endpoint_types.get(input), Some(EndpointType::Value)) {
            if let Some(slot) = self.values.get_mut(input) {
                *slot = value;
            }
            // Cancel any active ramp
            self.remove_active_ramp(input);
        }
    }

    /// Sets a value with ramping over the specified number of samples.
    pub fn set_value_with_ramp(&mut self, input: ValueKey, value: f32, ramp_samples: u32) {
        if !matches!(self.endpoint_types.get(input), Some(EndpointType::Value)) {
            return;
        }
        if ramp_samples == 0 {
            self.set_value(input, value);
            return;
        }

        // Compute step from current latched value
        let current = *self.values.get(input).unwrap_or(&0.0);
        let step = (value - current) / (ramp_samples as f32);

        if let Some(&idx) = self.ramp_indices.get(input) {
            if let Some(r) = self.active_ramps.get_mut(idx) {
                r.step = step;
                r.remaining = ramp_samples;
                r.target = value;
            }
        } else {
            let idx = self.active_ramps.len();
            self.active_ramps.push(ActiveRamp {
                key: input,
                step,
                remaining: ramp_samples,
                target: value,
            });
            self.ramp_indices.insert(input, idx);
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

    /// Build a node-level adjacency list from the value-level connections.
    /// Returns a map from each node to the nodes it connects to.
    fn build_node_adjacency(&self) -> HashMap<NodeKey, Vec<NodeKey>> {
        let mut adjacency: HashMap<NodeKey, Vec<NodeKey>> =
            HashMap::with_capacity(self.nodes.len());

        // Initialize empty adjacency lists for all nodes
        for node_key in self.nodes.keys() {
            adjacency.insert(node_key, Vec::new());
        }

        // Build adjacency from connections
        for (from_value, to_values) in self.connections.iter() {
            // Find which node owns the output value
            if let Some(&from_node) = self.value_to_node.get(from_value) {
                // Find which nodes own the input values
                for &to_value in to_values {
                    if let Some(&to_node) = self.value_to_node.get(to_value) {
                        // Add edge from_node -> to_node (avoiding duplicates)
                        let edges = adjacency.get_mut(&from_node).unwrap();
                        if !edges.contains(&to_node) {
                            edges.push(to_node);
                        }
                    }
                }
            }
        }

        adjacency
    }

    /// Perform topological sort with cycle detection.
    /// Returns the sorted node order or an error if an invalid cycle is detected.
    fn topological_sort(&mut self) -> Result<Vec<NodeKey>, GraphError> {
        let adjacency = self.build_node_adjacency();

        // Find all Delay nodes - we won't follow their outputs during sorting
        let delay_nodes: HashSet<NodeKey> = self
            .nodes
            .iter()
            .filter(|(_, data)| data.processor.allows_feedback())
            .map(|(key, _)| key)
            .collect();

        // Build modified adjacency for sorting - Delay nodes don't have outgoing edges
        let mut sort_adjacency = adjacency.clone();
        for &delay_node in &delay_nodes {
            sort_adjacency.insert(delay_node, Vec::new());
        }

        let mut sorted = Vec::with_capacity(self.nodes.len());
        let mut visited = HashSet::new();
        let mut recursion_stack = HashSet::new();

        // Helper function for DFS
        fn visit(
            node: NodeKey,
            adjacency: &HashMap<NodeKey, Vec<NodeKey>>,
            visited: &mut HashSet<NodeKey>,
            recursion_stack: &mut HashSet<NodeKey>,
            sorted: &mut Vec<NodeKey>,
        ) -> Result<(), GraphError> {
            if recursion_stack.contains(&node) {
                // Found a cycle - this should only happen if there's a cycle without delays
                let cycle = vec![node];
                return Err(GraphError::CycleDetected(cycle));
            }

            if visited.contains(&node) {
                return Ok(());
            }

            visited.insert(node);
            recursion_stack.insert(node);

            if let Some(neighbors) = adjacency.get(&node) {
                for &neighbor in neighbors {
                    visit(neighbor, adjacency, visited, recursion_stack, sorted)?;
                }
            }

            recursion_stack.remove(&node);
            sorted.push(node);

            Ok(())
        }

        // Visit all nodes
        for node in self.nodes.keys() {
            if !visited.contains(&node) {
                visit(
                    node,
                    &sort_adjacency,
                    &mut visited,
                    &mut recursion_stack,
                    &mut sorted,
                )?;
            }
        }

        // Reverse to get correct topological order
        sorted.reverse();

        // Now verify that any remaining cycles in the original graph have delays
        self.verify_cycles_have_delays(&adjacency)?;

        Ok(sorted)
    }

    /// Verify that all cycles in the graph contain at least one Delay node
    fn verify_cycles_have_delays(
        &self,
        adjacency: &HashMap<NodeKey, Vec<NodeKey>>,
    ) -> Result<(), GraphError> {
        let mut visited = HashSet::new();
        let mut recursion_stack = HashSet::new();
        let mut path = Vec::new();

        fn find_cycle(
            node: NodeKey,
            adjacency: &HashMap<NodeKey, Vec<NodeKey>>,
            visited: &mut HashSet<NodeKey>,
            recursion_stack: &mut HashSet<NodeKey>,
            path: &mut Vec<NodeKey>,
            nodes: &SlotMap<NodeKey, NodeData>,
        ) -> Result<(), GraphError> {
            visited.insert(node);
            recursion_stack.insert(node);
            path.push(node);

            if let Some(neighbors) = adjacency.get(&node) {
                for &neighbor in neighbors {
                    if !visited.contains(&neighbor) {
                        find_cycle(neighbor, adjacency, visited, recursion_stack, path, nodes)?;
                    } else if recursion_stack.contains(&neighbor) {
                        // Found a cycle
                        let cycle_start = path.iter().position(|&n| n == neighbor).unwrap();
                        let cycle_nodes: Vec<NodeKey> = path[cycle_start..].to_vec();

                        // Check if any node in the cycle allows feedback
                        let has_delay = cycle_nodes.iter().any(|&n| {
                            nodes
                                .get(n)
                                .map(|data| data.processor.allows_feedback())
                                .unwrap_or(false)
                        });

                        if !has_delay {
                            return Err(GraphError::CycleDetected(cycle_nodes));
                        }
                    }
                }
            }

            path.pop();
            recursion_stack.remove(&node);
            Ok(())
        }

        for node in self.nodes.keys() {
            if !visited.contains(&node) {
                find_cycle(
                    node,
                    adjacency,
                    &mut visited,
                    &mut recursion_stack,
                    &mut path,
                    &self.nodes,
                )?;
            }
        }

        Ok(())
    }

    /// Update the sorted node order if the topology is dirty.
    fn update_topology_if_needed(&mut self) -> Result<(), GraphError> {
        if self.topology_dirty {
            self.node_order = self.topological_sort()?;
            self.topology_dirty = false;
        }
        Ok(())
    }

    /// Validate the graph structure.
    /// This checks for invalid cycles and ensures the graph can be processed.
    /// Returns Ok(()) if the graph is valid, or an error describing the problem.
    pub fn validate(&mut self) -> Result<(), GraphError> {
        self.update_topology_if_needed()
    }

    /// Process one sample of audio for all nodes in the graph.
    ///
    /// This method:
    /// 1. Updates the topology if needed (sorts nodes)
    /// 2. Advances only active ramps and updates their latched values
    /// 3. Processes each node in topologically sorted order
    /// 4. Propagates output values to connected inputs
    /// 5. Handles any pending events in the event queue
    pub fn process(&mut self) -> Result<(), GraphError> {
        // Update topology if the graph structure has changed
        self.update_topology_if_needed()?;
        // Advance active ramps only
        let mut i = 0;
        while i < self.active_ramps.len() {
            let mut finished = false;
            if let Some(r) = self.active_ramps.get_mut(i) {
                if let Some(slot) = self.values.get_mut(r.key) {
                    *slot += r.step;
                }
                if r.remaining > 0 {
                    r.remaining -= 1;
                }
                if r.remaining == 0 {
                    // Snap to exact target and mark finished
                    if let Some(slot) = self.values.get_mut(r.key) {
                        *slot = r.target;
                    }
                    finished = true;
                }
            }

            if finished {
                // Remove ramp at index i via swap_remove
                let removed = self.active_ramps.swap_remove(i);
                // Clear index for removed key
                self.ramp_indices.remove(removed.key);
                // If we swapped in a new element at i, update its index mapping
                if i < self.active_ramps.len() {
                    let swapped_key = self.active_ramps[i].key;
                    if let Some(idx_slot) = self.ramp_indices.get_mut(swapped_key) {
                        *idx_slot = i;
                    } else {
                        // Should not happen, but ensure mapping exists
                        self.ramp_indices.insert(swapped_key, i);
                    }
                }
            } else {
                i += 1;
            }
        }

        // Process nodes in topologically sorted order
        for &node_key in &self.node_order {
            if let Some(node) = self.nodes.get_mut(node_key) {
                let mut input_values = ArrayVec::<f32, MAX_NODE_ENDPOINTS>::new();

                // Get input values
                for &input_key in &node.inputs {
                    input_values.push(self.values[input_key]);
                }

                // Process the node with its inputs to get the output
                let output = node.processor.process(self.sample_rate, &input_values);

                // Store the output value in the first output of the node
                if let Some(&output_key) = node.outputs.first() {
                    self.values[output_key] = output;

                    // Propagate the output to all connected inputs
                    if let Some(connections) = self.connections.get(output_key) {
                        for &target_input in connections {
                            self.values[target_input] = output;
                        }
                    }
                }

                // Process events
                while let Some((_target_input, _event)) = self.event_queue.pop() {
                    // TODO: Handle event processing
                    // Maybe add event handlers to the SignalProcessor trait?
                }
            }
        }

        Ok(())
    }

    fn remove_active_ramp(&mut self, key: ValueKey) {
        if let Some(&idx) = self.ramp_indices.get(key) {
            let removed = self.active_ramps.swap_remove(idx);
            self.ramp_indices.remove(removed.key);
            if idx < self.active_ramps.len() {
                let swapped_key = self.active_ramps[idx].key;
                if let Some(idx_slot) = self.ramp_indices.get_mut(swapped_key) {
                    *idx_slot = idx;
                } else {
                    self.ramp_indices.insert(swapped_key, idx);
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
            self.process()?;
            // Find the output key of the last node's first output
            // Note: This assumes the last node added is the final output node.
            // This might need refinement based on your desired graph structure.
            if let Some(output_key) = self
                .nodes
                .values()
                .last()
                .and_then(|node_data| node_data.outputs.first())
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

    /// Returns true if this node can be used to break feedback cycles.
    /// IMPORTANT: Only Delay nodes should return true. Incorrectly returning true
    /// for non-delay nodes will cause incorrect audio processing in feedback paths.
    fn allows_feedback(&self) -> bool {
        false
    }
}

// ProcessingNode is automatically implemented by the Node macro.
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
    fn input_endpoints(&self) -> &'static [EndpointMetadata] {
        const INPUTS: &[EndpointMetadata] = &[EndpointMetadata { name: "input", index: 0 }];
        INPUTS
    }

    fn output_endpoints(&self) -> &'static [EndpointMetadata] {
        const OUTPUTS: &[EndpointMetadata] = &[EndpointMetadata { name: "output", index: 0 }];
        OUTPUTS
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
    fn input_endpoints(&self) -> &'static [EndpointMetadata] {
        const INPUTS: &[EndpointMetadata] = &[
            EndpointMetadata { name: "input1", index: 0 },
            EndpointMetadata { name: "input2", index: 1 },
        ];
        INPUTS
    }

    fn output_endpoints(&self) -> &'static [EndpointMetadata] {
        const OUTPUTS: &[EndpointMetadata] = &[EndpointMetadata { name: "output", index: 0 }];
        OUTPUTS
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::delay::Delay;
    use crate::filters::tpt::TptFilter;
    use crate::oscillators::Oscillator;

    #[test]
    fn test_simple_chain_topology() {
        let mut graph = Graph::new(44100.0);

        // Create a simple chain: osc -> filter
        let osc = graph.add_node(Oscillator::sine(440.0, 1.0));
        let filter = graph.add_node(TptFilter::new(1000.0, 0.7));

        graph.connect(osc.output(), filter.input());

        // Validate should succeed
        assert!(graph.validate().is_ok());

        // Process should work
        assert!(graph.process().is_ok());
    }

    #[test]
    fn test_invalid_cycle_without_delay() {
        let mut graph = Graph::new(44100.0);

        // Create a cycle without delay: osc -> filter -> osc
        let osc = graph.add_node(Oscillator::sine(440.0, 1.0));
        let filter = graph.add_node(TptFilter::new(1000.0, 0.7));

        graph.connect(osc.output(), filter.input());
        graph.connect(filter.output(), osc.frequency());

        // Validation should fail - cycle without delay
        assert!(graph.validate().is_err());
        if let Err(GraphError::CycleDetected(nodes)) = graph.validate() {
            assert!(nodes.len() > 0);
        }
    }

    #[test]
    fn test_valid_cycle_with_delay() {
        let mut graph = Graph::new(44100.0);

        // Create a cycle with delay: osc -> filter -> delay -> osc
        let osc = graph.add_node(Oscillator::sine(440.0, 1.0));
        let filter = graph.add_node(TptFilter::new(1000.0, 0.7));
        let delay = graph.add_node(Delay::new(0.5, 0.3));

        graph.connect(osc.output(), filter.input());
        graph.connect(filter.output(), delay.input());
        graph.connect(delay.output(), osc.frequency());

        assert!(graph.validate().is_ok());

        assert!(graph.process().is_ok());
    }

    #[test]
    fn test_nodes_added_out_of_order() {
        let mut graph = Graph::new(44100.0);

        // Add nodes in reverse dependency order
        let filter = graph.add_node(TptFilter::new(1000.0, 0.7));
        let osc = graph.add_node(Oscillator::sine(440.0, 1.0));

        graph.connect(osc.output(), filter.input());

        // Should still work thanks to topological sorting
        assert!(graph.validate().is_ok());
        assert!(graph.process().is_ok());
    }

    #[test]
    fn test_complex_graph_with_multiple_paths() {
        let mut graph = Graph::new(44100.0);

        // Create a more complex graph:
        //     osc1 -> filter1 -> output
        //     osc2 -> filter2 -> output
        let osc1 = graph.add_node(Oscillator::sine(440.0, 1.0));
        let osc2 = graph.add_node(Oscillator::sine(880.0, 1.0));
        let filter1 = graph.add_node(TptFilter::new(1000.0, 0.7));
        let filter2 = graph.add_node(TptFilter::new(2000.0, 0.5));

        graph.connect(osc1.output(), filter1.input());
        graph.connect(osc2.output(), filter2.input());

        // Both paths should be processed correctly
        assert!(graph.validate().is_ok());
        assert!(graph.process().is_ok());
    }
}
