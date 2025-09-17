use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::fmt;

use arrayvec::ArrayVec;
use hound;
use slotmap::{SecondaryMap, SlotMap};

use super::audio_input::AudioInput;
use super::helpers::{BinaryFunctionNode, FunctionNode};
use super::traits::{ProcessingNode, SignalProcessor};
use super::types::{
    Connection, ConnectionBuilder, EndpointType, InputEndpoint, NodeKey, OutputEndpoint, ValueKey,
    MAX_CONNECTIONS_PER_OUTPUT, MAX_NODE_ENDPOINTS,
};

pub struct NodeData {
    pub processor: Box<dyn SignalProcessor>,
    pub inputs: ArrayVec<ValueKey, MAX_NODE_ENDPOINTS>,
    pub outputs: ArrayVec<ValueKey, MAX_NODE_ENDPOINTS>,
}

#[derive(Debug, Clone)]
pub enum GraphError {
    CycleDetected(Vec<NodeKey>),
}

impl fmt::Display for GraphError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GraphError::CycleDetected(nodes) => {
                write!(
                    f,
                    "Invalid cycle detected in graph. Cycles must contain at least one Delay node. Cycle contains {} nodes",
                    nodes.len()
                )
            }
        }
    }
}

impl Error for GraphError {}

pub struct Graph {
    pub sample_rate: f32,
    pub nodes: SlotMap<NodeKey, NodeData>,
    pub values: SlotMap<ValueKey, f32>,
    pub connections: SecondaryMap<ValueKey, ArrayVec<ValueKey, MAX_CONNECTIONS_PER_OUTPUT>>,
    pub endpoint_types: SecondaryMap<ValueKey, EndpointType>,
    node_order: Vec<NodeKey>,
    topology_dirty: bool,
    value_to_node: SecondaryMap<ValueKey, NodeKey>,
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
            node_order: Vec::new(),
            topology_dirty: true,
            value_to_node: SecondaryMap::new(),
            active_ramps: Vec::new(),
            ramp_indices: SecondaryMap::new(),
        }
    }

    /// Adds a processing node by initializing it, allocating value slots for its declared
    /// endpoints, and storing the boxed processor; the node-specific endpoint handle produced
    /// by `ProcessingNode::create_endpoints` is returned for ergonomic graph wiring.
    pub fn add_node<T: ProcessingNode + 'static>(&mut self, mut node: T) -> T::Endpoints {
        node.init(self.sample_rate);

        let mut inputs = ArrayVec::<ValueKey, MAX_NODE_ENDPOINTS>::new();
        for endpoint_type in T::INPUT_TYPES.iter() {
            let key = self.values.insert(0.0);
            self.endpoint_types.insert(key, *endpoint_type);
            inputs.push(key);
        }

        let mut outputs = ArrayVec::<ValueKey, MAX_NODE_ENDPOINTS>::new();
        for endpoint_type in T::OUTPUT_TYPES.iter() {
            let key = self.values.insert(0.0);
            self.endpoint_types.insert(key, *endpoint_type);
            outputs.push(key);
        }

        let node_key = self.nodes.insert(NodeData {
            processor: Box::new(node),
            inputs: inputs.clone(),
            outputs: outputs.clone(),
        });

        for &value_key in inputs.iter().chain(outputs.iter()) {
            self.value_to_node.insert(value_key, node_key);
        }

        self.topology_dirty = true;

        T::create_endpoints(node_key, inputs, outputs)
    }

    pub fn add_audio_input(&mut self) -> (<AudioInput as ProcessingNode>::Endpoints, ValueKey) {
        let input_node = self.add_node(AudioInput::new());
        let input_key = self
            .insert_value_input(input_node.input_value(), 0.0)
            .expect("Failed to insert audio input value");
        (input_node, input_key)
    }

    //TODO: should this return type be Option or Result?
    pub fn get_input(&self, node: NodeKey, index: usize) -> Option<ValueKey> {
        self.nodes
            .get(node)
            .and_then(|node_data| node_data.inputs.get(index))
            .copied()
    }

    pub fn get_node_output(&self, node: NodeKey, index: usize) -> Option<ValueKey> {
        self.nodes
            .get(node)
            .and_then(|node_data| node_data.outputs.get(index))
            .copied()
    }

    pub fn insert_value_input(
        &mut self,
        input: InputEndpoint,
        initial_value: f32,
    ) -> Option<ValueKey> {
        let key = input.key();
        let value = self.values.get_mut(key)?;
        if let Some(existing) = self.endpoint_types.get(key) {
            if *existing != EndpointType::Value {
                return None;
            }
        }
        *value = initial_value;
        self.endpoint_types.insert(key, EndpointType::Value);
        self.remove_active_ramp(key);
        Some(key)
    }

    pub fn connect(&mut self, from: OutputEndpoint, to: InputEndpoint) {
        self.connections
            .entry(from.key())
            .unwrap()
            .or_default()
            .push(to.key());

        self.topology_dirty = true;
    }

    pub fn connect_all(&mut self, connections: Vec<ConnectionBuilder>) {
        for builder in connections {
            for Connection { from, to } in builder.connections {
                self.connect(from, to);
            }
        }
    }

    pub fn transform(&mut self, from: OutputEndpoint, f: fn(f32) -> f32) -> OutputEndpoint {
        let node = FunctionNode::new(f);
        let processor: Box<dyn SignalProcessor> = Box::new(node);

        let input_key = self.values.insert(0.0);
        self.endpoint_types.insert(input_key, EndpointType::Stream);
        let mut input_keys = ArrayVec::new();
        input_keys.push(input_key);

        let output_key = self.values.insert(0.0);
        self.endpoint_types.insert(output_key, EndpointType::Stream);
        let mut output_keys = ArrayVec::new();
        output_keys.push(output_key);

        let node_key = self.nodes.insert(NodeData {
            processor,
            inputs: input_keys.clone(),
            outputs: output_keys.clone(),
        });

        for &value_key in input_keys.iter().chain(output_keys.iter()) {
            self.value_to_node.insert(value_key, node_key);
        }

        self.topology_dirty = true;

        let output = OutputEndpoint::new(output_key);

        self.connect(from, InputEndpoint::new(input_key));

        output
    }

    pub fn combine(
        &mut self,
        from1: OutputEndpoint,
        from2: OutputEndpoint,
        f: fn(f32, f32) -> f32,
    ) -> OutputEndpoint {
        let node = BinaryFunctionNode::new(f);
        let processor: Box<dyn SignalProcessor> = Box::new(node);

        let input_key1 = self.values.insert(0.0);
        self.endpoint_types.insert(input_key1, EndpointType::Stream);
        let input_key2 = self.values.insert(0.0);
        self.endpoint_types.insert(input_key2, EndpointType::Stream);
        let mut input_keys = ArrayVec::new();
        input_keys.push(input_key1);
        input_keys.push(input_key2);

        let output_key = self.values.insert(0.0);
        self.endpoint_types.insert(output_key, EndpointType::Stream);
        let mut output_keys = ArrayVec::new();
        output_keys.push(output_key);

        let node_key = self.nodes.insert(NodeData {
            processor,
            inputs: input_keys.clone(),
            outputs: output_keys.clone(),
        });

        for &value_key in input_keys.iter().chain(output_keys.iter()) {
            self.value_to_node.insert(value_key, node_key);
        }

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

    pub fn set_value(&mut self, input: ValueKey, value: f32) {
        if matches!(self.endpoint_types.get(input), Some(EndpointType::Value)) {
            if let Some(slot) = self.values.get_mut(input) {
                *slot = value;
            }
            self.remove_active_ramp(input);
        }
    }

    pub fn set_value_with_ramp(&mut self, input: ValueKey, value: f32, ramp_samples: u32) {
        if !matches!(self.endpoint_types.get(input), Some(EndpointType::Value)) {
            return;
        }
        if ramp_samples == 0 {
            self.set_value(input, value);
            return;
        }

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

    pub fn process(&mut self) -> Result<(), GraphError> {
        self.update_topology_if_needed()?;

        let mut i = 0;
        while i < self.active_ramps.len() {
            let mut finished_key: Option<ValueKey> = None;
            if let Some(r) = self.active_ramps.get_mut(i) {
                if let Some(slot) = self.values.get_mut(r.key) {
                    *slot += r.step;
                }
                if r.remaining > 0 {
                    r.remaining -= 1;
                }
                if r.remaining == 0 {
                    if let Some(slot) = self.values.get_mut(r.key) {
                        *slot = r.target;
                    }
                    finished_key = Some(r.key);
                }
            }

            if let Some(key) = finished_key {
                self.remove_active_ramp(key);
            } else {
                i += 1;
            }
        }

        for &node_key in &self.node_order {
            if let Some(node) = self.nodes.get_mut(node_key) {
                let mut input_values = ArrayVec::<f32, MAX_NODE_ENDPOINTS>::new();

                for &input_key in &node.inputs {
                    input_values.push(self.values[input_key]);
                }

                let output = node.processor.process(self.sample_rate, &input_values);

                if let Some(&output_key) = node.outputs.first() {
                    self.values[output_key] = output;

                    if let Some(connections) = self.connections.get(output_key) {
                        for &target_input in connections {
                            self.values[target_input] = output;
                        }
                    }
                }
            }
        }

        Ok(())
    }

    fn update_topology_if_needed(&mut self) -> Result<(), GraphError> {
        if self.topology_dirty {
            self.node_order = self.topological_sort()?;
            self.topology_dirty = false;
        }
        Ok(())
    }

    fn build_node_adjacency(&self) -> HashMap<NodeKey, Vec<NodeKey>> {
        let mut adjacency: HashMap<NodeKey, Vec<NodeKey>> =
            HashMap::with_capacity(self.nodes.len());

        for node_key in self.nodes.keys() {
            adjacency.insert(node_key, Vec::new());
        }

        for (from_value, to_values) in self.connections.iter() {
            if let Some(&from_node) = self.value_to_node.get(from_value) {
                for &to_value in to_values {
                    if let Some(&to_node) = self.value_to_node.get(to_value) {
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

    fn topological_sort(&mut self) -> Result<Vec<NodeKey>, GraphError> {
        let adjacency = self.build_node_adjacency();

        let delay_nodes: HashSet<NodeKey> = self
            .nodes
            .iter()
            .filter(|(_, data)| data.processor.allows_feedback())
            .map(|(key, _)| key)
            .collect();

        let mut sort_adjacency = adjacency.clone();
        for &delay_node in &delay_nodes {
            sort_adjacency.insert(delay_node, Vec::new());
        }

        let mut sorted = Vec::with_capacity(self.nodes.len());
        let mut visited = HashSet::new();
        let mut recursion_stack = HashSet::new();

        fn visit(
            node: NodeKey,
            adjacency: &HashMap<NodeKey, Vec<NodeKey>>,
            visited: &mut HashSet<NodeKey>,
            recursion_stack: &mut HashSet<NodeKey>,
            sorted: &mut Vec<NodeKey>,
        ) -> Result<(), GraphError> {
            if recursion_stack.contains(&node) {
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

        sorted.reverse();
        self.verify_cycles_have_delays(&adjacency)?;

        Ok(sorted)
    }

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
                        let cycle_start = path.iter().position(|&n| n == neighbor).unwrap();
                        let cycle_nodes: Vec<NodeKey> = path[cycle_start..].to_vec();

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

            recursion_stack.remove(&node);
            path.pop();
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
            channels: 2,
            sample_rate: self.sample_rate as u32,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        };

        let mut writer = hound::WavWriter::create(path, spec)?;
        let num_samples = (duration_secs * self.sample_rate) as u32;

        for _ in 0..num_samples {
            self.process()?;
            if let Some(output_key) = self
                .nodes
                .values()
                .last()
                .and_then(|node_data| node_data.outputs.first())
                .copied()
            {
                if let Some(&value) = self.values.get(output_key) {
                    writer.write_sample(value)?;
                    writer.write_sample(value)?;
                }
            }
        }

        writer.finalize()?;
        Ok(())
    }

    pub fn validate(&mut self) -> Result<(), GraphError> {
        self.update_topology_if_needed()
    }
}
