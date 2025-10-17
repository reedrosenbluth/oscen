use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::fmt;

use arrayvec::ArrayVec;
use hound;
use slotmap::{SecondaryMap, SlotMap};

use super::audio_input::AudioInput;
use super::helpers::{BinaryFunctionNode, FunctionNode};
use super::traits::{PendingEvent, ProcessingContext, ProcessingNode, SignalProcessor};
use super::types::{
    Connection, ConnectionBuilder, EndpointDescriptor, EndpointDirection, EndpointState,
    EndpointType, EventInstance, EventParam, EventPayload, InputEndpoint, NodeKey, Output,
    StreamOutput, ValueData, ValueInput, ValueKey, ValueParam, MAX_CONNECTIONS_PER_OUTPUT,
    MAX_NODE_ENDPOINTS,
};

impl fmt::Debug for NodeData {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NodeData")
            .field("processor", &"<SignalProcessor>")
            .field("inputs", &self.inputs)
            .field("outputs", &self.outputs)
            .finish()
    }
}

pub struct NodeData {
    pub processor: Box<dyn SignalProcessor>,
    pub inputs: ArrayVec<ValueKey, MAX_NODE_ENDPOINTS>,
    pub outputs: ArrayVec<ValueKey, MAX_NODE_ENDPOINTS>,
    pub input_types: ArrayVec<EndpointType, MAX_NODE_ENDPOINTS>,
    pub output_types: ArrayVec<EndpointType, MAX_NODE_ENDPOINTS>,
    pub has_event_inputs: bool,
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

#[derive(Debug)]
pub struct Graph {
    pub sample_rate: f32,
    pub nodes: SlotMap<NodeKey, NodeData>,
    pub endpoints: SlotMap<ValueKey, EndpointState>,
    pub connections: SecondaryMap<ValueKey, ArrayVec<ValueKey, MAX_CONNECTIONS_PER_OUTPUT>>,
    pub endpoint_types: SecondaryMap<ValueKey, EndpointType>,
    pub endpoint_descriptors: SecondaryMap<ValueKey, &'static EndpointDescriptor>,
    node_order: Vec<NodeKey>,
    topology_dirty: bool,
    value_to_node: SecondaryMap<ValueKey, NodeKey>,
    active_ramps: Vec<ActiveRamp>,
    ramp_indices: SecondaryMap<ValueKey, usize>,
    current_frame: u32,
    pending_events: Vec<PendingEvent>,
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
            endpoints: SlotMap::with_key(),
            connections: SecondaryMap::new(),
            endpoint_types: SecondaryMap::new(),
            endpoint_descriptors: SecondaryMap::new(),
            node_order: Vec::new(),
            topology_dirty: true,
            value_to_node: SecondaryMap::new(),
            active_ramps: Vec::with_capacity(32),
            ramp_indices: SecondaryMap::new(),
            current_frame: 0,
            pending_events: Vec::with_capacity(64),
        }
    }

    /// Adds a processing node by initializing it, allocating value slots for its declared
    /// endpoints, and storing the boxed processor; the node-specific endpoint handle produced
    /// by `ProcessingNode::create_endpoints` is returned for ergonomic graph wiring.
    pub fn add_node<T: ProcessingNode + 'static>(&mut self, mut node: T) -> T::Endpoints {
        node.init(self.sample_rate);

        let mut inputs = ArrayVec::<ValueKey, MAX_NODE_ENDPOINTS>::new();
        let mut outputs = ArrayVec::<ValueKey, MAX_NODE_ENDPOINTS>::new();
        let mut input_types = ArrayVec::<EndpointType, MAX_NODE_ENDPOINTS>::new();
        let mut output_types = ArrayVec::<EndpointType, MAX_NODE_ENDPOINTS>::new();
        let mut has_event_inputs = false;

        for descriptor in T::ENDPOINT_DESCRIPTORS.iter() {
            let key = self.allocate_endpoint(descriptor.endpoint_type);
            match descriptor.direction {
                EndpointDirection::Input => {
                    inputs.push(key);
                    input_types.push(descriptor.endpoint_type);
                    if descriptor.endpoint_type == EndpointType::Event {
                        has_event_inputs = true;
                    }
                }
                EndpointDirection::Output => {
                    outputs.push(key);
                    output_types.push(descriptor.endpoint_type);
                }
            }
            self.set_endpoint_descriptor(key, descriptor);
        }

        // Initialize value inputs with default values from the node
        for (idx, value) in node.default_values() {
            if let Some(&key) = inputs.get(idx) {
                self.insert_value_input(ValueInput::new(InputEndpoint::new(key)), value);
            }
        }

        let node_key = self.nodes.insert(NodeData {
            processor: Box::new(node),
            inputs: inputs.clone(),
            outputs: outputs.clone(),
            input_types,
            output_types,
            has_event_inputs,
        });

        for &value_key in inputs.iter().chain(outputs.iter()) {
            self.value_to_node.insert(value_key, node_key);
        }

        self.topology_dirty = true;

        T::create_endpoints(node_key, inputs, outputs)
    }

    pub fn add_audio_input(
        &mut self,
    ) -> (<AudioInput as ProcessingNode>::Endpoints, ValueInput) {
        let input_node = self.add_node(AudioInput::new());
        let input_handle = input_node.input_value;
        self.insert_value_input(input_handle, 0.0)
            .expect("Failed to insert audio input value");
        (input_node, input_handle)
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

    pub fn insert_value_input(&mut self, input: ValueInput, initial_value: f32) -> Option<ValueKey> {
        let key: ValueKey = input.into();
        if let Some(existing) = self.endpoint_types.get(key) {
            if *existing != EndpointType::Value {
                return None;
            }
        }

        let endpoint = self.endpoints.get_mut(key)?;
        match endpoint {
            EndpointState::Value(value) => {
                value.set_scalar(initial_value);
                self.endpoint_types.insert(key, EndpointType::Value);
                self.remove_active_ramp(key);
                Some(key)
            }
            //TODO: error here?
            EndpointState::Stream(_) | EndpointState::Event(_) => None,
        }
    }

    pub fn connect<O, I>(&mut self, from: O, to: I)
    where
        O: Output,
        I: Into<InputEndpoint>,
    {
        let to_endpoint = to.into();

        self.connections
            .entry(from.key())
            .unwrap()
            .or_default()
            .push(to_endpoint.key());

        self.topology_dirty = true;
    }

    pub fn connect_all(&mut self, connections: Vec<ConnectionBuilder>) {
        for builder in connections {
            for Connection { from, to } in builder.connections {
                self.connections
                    .entry(from)
                    .unwrap()
                    .or_default()
                    .push(to);
                self.topology_dirty = true;
            }
        }
    }

    pub fn set_endpoint_descriptor(
        &mut self,
        key: ValueKey,
        descriptor: &'static EndpointDescriptor,
    ) {
        self.endpoint_descriptors.insert(key, descriptor);
    }

    pub fn endpoint_descriptor(&self, key: ValueKey) -> Option<&EndpointDescriptor> {
        self.endpoint_descriptors.get(key).copied()
    }

    pub fn transform<O>(&mut self, from: O, f: fn(f32) -> f32) -> StreamOutput
    where
        O: Output,
    {
        let node = FunctionNode::new(f);
        let processor: Box<dyn SignalProcessor> = Box::new(node);

        let input_key = self.allocate_endpoint(EndpointType::Stream);
        let mut input_keys = ArrayVec::new();
        input_keys.push(input_key);

        let output_key = self.allocate_endpoint(EndpointType::Stream);
        let mut output_keys = ArrayVec::new();
        output_keys.push(output_key);

        let mut input_types = ArrayVec::new();
        input_types.push(EndpointType::Stream);

        let mut output_types = ArrayVec::new();
        output_types.push(EndpointType::Stream);

        let node_key = self.nodes.insert(NodeData {
            processor,
            inputs: input_keys.clone(),
            outputs: output_keys.clone(),
            input_types,
            output_types,
            has_event_inputs: false,
        });

        for &value_key in input_keys.iter().chain(output_keys.iter()) {
            self.value_to_node.insert(value_key, node_key);
        }

        self.topology_dirty = true;

        let output = StreamOutput::new(output_key);

        self.connect(from, InputEndpoint::new(input_key));

        output
    }

    pub fn combine<O1, O2>(
        &mut self,
        from1: O1,
        from2: O2,
        f: fn(f32, f32) -> f32,
    ) -> StreamOutput
    where
        O1: Output,
        O2: Output,
    {
        let node = BinaryFunctionNode::new(f);
        let processor: Box<dyn SignalProcessor> = Box::new(node);

        let input_key1 = self.allocate_endpoint(EndpointType::Stream);
        let input_key2 = self.allocate_endpoint(EndpointType::Stream);
        let mut input_keys = ArrayVec::new();
        input_keys.push(input_key1);
        input_keys.push(input_key2);

        let output_key = self.allocate_endpoint(EndpointType::Stream);
        let mut output_keys = ArrayVec::new();
        output_keys.push(output_key);

        let mut input_types = ArrayVec::new();
        input_types.push(EndpointType::Stream);
        input_types.push(EndpointType::Stream);

        let mut output_types = ArrayVec::new();
        output_types.push(EndpointType::Stream);

        let node_key = self.nodes.insert(NodeData {
            processor,
            inputs: input_keys.clone(),
            outputs: output_keys.clone(),
            input_types,
            output_types,
            has_event_inputs: false,
        });

        for &value_key in input_keys.iter().chain(output_keys.iter()) {
            self.value_to_node.insert(value_key, node_key);
        }

        self.topology_dirty = true;

        let output = StreamOutput::new(output_key);

        self.connect(from1, InputEndpoint::new(input_key1));
        self.connect(from2, InputEndpoint::new(input_key2));

        output
    }

    pub fn multiply<O1, O2>(&mut self, a: O1, b: O2) -> StreamOutput
    where
        O1: Output,
        O2: Output,
    {
        self.combine(a, b, |x, y| x * y)
    }

    pub fn add<O1, O2>(&mut self, a: O1, b: O2) -> StreamOutput
    where
        O1: Output,
        O2: Output,
    {
        self.combine(a, b, |x, y| x + y)
    }

    pub fn set_value<I>(&mut self, input: I, value: f32)
    where
        I: Into<ValueKey>,
    {
        let key = input.into();

        if matches!(self.endpoint_types.get(key), Some(EndpointType::Value)) {
            if let Some(state) = self.endpoints.get_mut(key) {
                state.set_scalar(value);
            }
            self.remove_active_ramp(key);
        }
    }

    /// Convenience method for updating a ValueParam
    pub fn set_param(&mut self, param: ValueParam, value: f32) {
        self.set_value(param.input, value);
    }

    /// Create a value parameter node and return an opaque handle that can be both
    /// updated via `set_param` and connected to other nodes.
    pub fn value_param(&mut self, default: f32) -> ValueParam {
        use crate::value::Value;

        let node = self.add_node(Value::new(default));
        let input = node.input;
        self.insert_value_input(input, default);
        ValueParam::new(input, node.output)
    }

    /// Create an event parameter node and return an opaque handle that can be both
    /// queued via `queue_event` and connected to other nodes.
    pub fn event_param(&mut self) -> EventParam {
        use crate::event_passthrough::EventPassthrough;

        let node = self.add_node(EventPassthrough::new());
        EventParam::new(node.input, node.output)
    }

    pub fn queue_event<I>(&mut self, input: I, frame_offset: u32, payload: EventPayload) -> bool
    where
        I: Into<InputEndpoint>,
    {
        let key = input.into().key();

        if !matches!(self.endpoint_types.get(key), Some(EndpointType::Event)) {
            return false;
        }

        if let Some(state) = self.endpoints.get_mut(key) {
            if let Some(event_state) = state.as_event_mut() {
                return event_state.queue_mut().push(EventInstance {
                    frame_offset,
                    payload,
                });
            }
        }

        false
    }

    pub fn drain_events<O, F>(&mut self, output: O, mut handler: F)
    where
        O: Output,
        F: FnMut(&EventInstance),
    {
        let key = output.key();

        if !matches!(self.endpoint_types.get(key), Some(EndpointType::Event)) {
            return;
        }

        if let Some(state) = self.endpoints.get_mut(key) {
            if let Some(event_state) = state.as_event_mut() {
                let queue = event_state.queue_mut();
                for event in queue.events() {
                    handler(event);
                }
                queue.clear();
            }
        }
    }

    pub fn set_value_with_ramp<I>(&mut self, input: I, value: f32, ramp_samples: u32)
    where
        I: Into<ValueKey>,
    {
        let key = input.into();

        if !matches!(self.endpoint_types.get(key), Some(EndpointType::Value)) {
            return;
        }
        if ramp_samples == 0 {
            self.set_value(key, value);
            return;
        }

        let current = self
            .endpoints
            .get(key)
            .and_then(EndpointState::as_scalar)
            .unwrap_or(0.0);
        let step = (value - current) / (ramp_samples as f32);

        if let Some(&idx) = self.ramp_indices.get(key) {
            if let Some(r) = self.active_ramps.get_mut(idx) {
                r.step = step;
                r.remaining = ramp_samples;
                r.target = value;
            }
        } else {
            let idx = self.active_ramps.len();
            self.active_ramps.push(ActiveRamp {
                key,
                step,
                remaining: ramp_samples,
                target: value,
            });
            self.ramp_indices.insert(key, idx);
        }
    }

    pub fn get_value<O>(&self, endpoint: &O) -> Option<f32>
    where
        O: Output,
    {
        self.endpoints
            .get(endpoint.key())
            .and_then(EndpointState::as_scalar)
    }

    pub fn process(&mut self) -> Result<(), GraphError> {
        self.update_topology_if_needed()?;

        let mut i = 0;
        while i < self.active_ramps.len() {
            let mut finished_key: Option<ValueKey> = None;
            if let Some(r) = self.active_ramps.get_mut(i) {
                if let Some(state) = self.endpoints.get_mut(r.key) {
                    if let Some(slot) = state.as_scalar_mut() {
                        *slot += r.step;
                    }
                }
                if r.remaining > 0 {
                    r.remaining -= 1;
                }
                if r.remaining == 0 {
                    if let Some(state) = self.endpoints.get_mut(r.key) {
                        if let Some(slot) = state.as_scalar_mut() {
                            *slot = r.target;
                        }
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

        // Use index-based iteration to avoid cloning node_order
        for node_idx in 0..self.node_order.len() {
            let node_key = self.node_order[node_idx];

            {
                if let Some(node) = self.nodes.get_mut(node_key) {
                    let output = {
                    // Use fixed arrays instead of ArrayVec to reduce initialization overhead
                    let mut input_values: [f32; MAX_NODE_ENDPOINTS] = [0.0; MAX_NODE_ENDPOINTS];
                    let mut value_inputs: [Option<&ValueData>; MAX_NODE_ENDPOINTS] =
                        [None; MAX_NODE_ENDPOINTS];
                    let mut event_inputs: [&[EventInstance]; MAX_NODE_ENDPOINTS] =
                        [&[]; MAX_NODE_ENDPOINTS];

                    let num_inputs = node.inputs.len();

                    // Use cached input types instead of SecondaryMap lookup
                    for idx in 0..num_inputs {
                        let input_key = node.inputs[idx];
                        let endpoint_type = node.input_types[idx];

                        match endpoint_type {
                            EndpointType::Event => {
                                let endpoint_state = self.endpoints.get(input_key);
                                event_inputs[idx] = endpoint_state
                                    .and_then(EndpointState::as_event)
                                    .map(|state| state.queue().events())
                                    .unwrap_or(&[]);
                            }
                            EndpointType::Stream => {
                                let endpoint_state = self.endpoints.get(input_key);
                                input_values[idx] = endpoint_state
                                    .and_then(EndpointState::as_scalar)
                                    .unwrap_or(0.0);
                            }
                            EndpointType::Value => {
                                let endpoint_state = self.endpoints.get(input_key);
                                value_inputs[idx] = endpoint_state.and_then(|state| {
                                    if let EndpointState::Value(data) = state {
                                        Some(data)
                                    } else {
                                        None
                                    }
                                });
                                input_values[idx] = endpoint_state
                                    .and_then(EndpointState::as_scalar)
                                    .unwrap_or(0.0);
                            }
                        }
                    }

                    self.pending_events.clear();

                    let mut context = ProcessingContext::new(
                        &input_values[..num_inputs],
                        &value_inputs[..num_inputs],
                        &event_inputs[..num_inputs],
                        &mut self.pending_events,
                    );

                    node.processor.process(self.sample_rate, &mut context)
                };

                if let Some(&output_key) = node.outputs.first() {
                    if let Some(state) = self.endpoints.get_mut(output_key) {
                        state.set_scalar(output);
                    }

                    if let Some(connections) = self.connections.get(output_key) {
                        for &target_input in connections {
                            if let Some(target_state) = self.endpoints.get_mut(target_input) {
                                target_state.set_scalar(output);
                            }
                        }
                    }
                }

                // Process events if any were emitted
                if !self.pending_events.is_empty() {
                    for pending in self.pending_events.iter() {
                        let output_idx = pending.output_index;

                        // Use cached output type instead of SecondaryMap lookup
                        if let Some(&output_type) = node.output_types.get(output_idx) {
                            if output_type != EndpointType::Event {
                                continue;
                            }

                            if let Some(&event_output_key) = node.outputs.get(output_idx) {
                                if let Some(state) = self.endpoints.get_mut(event_output_key) {
                                    if let Some(event_state) = state.as_event_mut() {
                                        let _ = event_state.queue_mut().push(pending.event.clone());
                                    }
                                }

                                if let Some(targets) = self.connections.get(event_output_key) {
                                    for &target_input in targets {
                                        if let Some(target_state) = self.endpoints.get_mut(target_input)
                                        {
                                            if let Some(event_state) = target_state.as_event_mut() {
                                                let _ =
                                                    event_state.queue_mut().push(pending.event.clone());
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                self.pending_events.clear();

                // Only clear event queues if this node has event inputs (cached flag)
                if node.has_event_inputs {
                    for (idx, &input_key) in node.inputs.iter().enumerate() {
                        if node.input_types[idx] == EndpointType::Event {
                            if let Some(state) = self.endpoints.get_mut(input_key) {
                                if let Some(event_state) = state.as_event_mut() {
                                    event_state.queue_mut().clear();
                                }
                            }
                        }
                    }
                }
                }
            }
        }

        self.current_frame = self.current_frame.wrapping_add(1);

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

    pub fn allocate_endpoint(&mut self, endpoint_type: EndpointType) -> ValueKey {
        let state = match endpoint_type {
            EndpointType::Stream => EndpointState::stream(0.0),
            EndpointType::Value => EndpointState::value(0.0),
            EndpointType::Event => EndpointState::event(),
        };

        let key = self.endpoints.insert(state);
        self.endpoint_types.insert(key, endpoint_type);
        self.endpoint_descriptors.remove(key);

        key
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
                if let Some(value) = self
                    .endpoints
                    .get(output_key)
                    .and_then(EndpointState::as_scalar)
                {
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

    /// Check if a specific node in this graph is active
    pub fn is_node_active(&self, node_key: NodeKey) -> bool {
        self.nodes
            .get(node_key)
            .map(|node| node.processor.is_active())
            .unwrap_or(true)
    }
}
