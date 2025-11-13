use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::fmt;

use arrayvec::ArrayVec;
use hound;
use slotmap::{SecondaryMap, SlotMap};

use super::audio_input::AudioInput;
use super::helpers::{BinaryFunctionNode, FunctionNode};
use super::traits::{
    IOStructAccess, PendingEvent, ProcessingContext, ProcessingNode, SignalProcessor,
};
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
    pub num_stream_inputs: usize,
    pub num_stream_outputs: usize,
    pub create_io_fn: fn() -> Box<dyn IOStructAccess>,
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

/// Dynamic IO struct for use in Graph::process()
/// Stores stream I/O data in fixed-size arrays and implements IOStructAccess
pub struct DynamicIO {
    stream_inputs: [f32; MAX_NODE_ENDPOINTS],
    stream_outputs: [f32; MAX_NODE_ENDPOINTS],
    num_stream_inputs: usize,
    num_stream_outputs: usize,
}

impl DynamicIO {
    pub fn new(num_stream_inputs: usize, num_stream_outputs: usize) -> Self {
        Self {
            stream_inputs: [0.0; MAX_NODE_ENDPOINTS],
            stream_outputs: [0.0; MAX_NODE_ENDPOINTS],
            num_stream_inputs,
            num_stream_outputs,
        }
    }
}

impl IOStructAccess for DynamicIO {
    #[inline]
    fn num_stream_inputs(&self) -> usize {
        self.num_stream_inputs
    }

    #[inline]
    fn num_stream_outputs(&self) -> usize {
        self.num_stream_outputs
    }

    #[inline]
    fn num_event_outputs(&self) -> usize {
        0 // Events still go through context for now
    }

    #[inline]
    fn set_stream_input(&mut self, index: usize, value: f32) {
        if index < self.num_stream_inputs {
            self.stream_inputs[index] = value;
        }
    }

    #[inline]
    fn get_stream_input(&self, index: usize) -> Option<f32> {
        if index < self.num_stream_inputs {
            Some(self.stream_inputs[index])
        } else {
            None
        }
    }

    #[inline]
    fn set_stream_output(&mut self, index: usize, value: f32) {
        if index < self.num_stream_outputs {
            self.stream_outputs[index] = value;
        }
    }

    #[inline]
    fn get_stream_output(&self, index: usize) -> Option<f32> {
        if index < self.num_stream_outputs {
            Some(self.stream_outputs[index])
        } else {
            None
        }
    }

    #[inline]
    fn get_event_output(&self, _index: usize) -> &[EventInstance] {
        &[] // Events still go through context for now
    }

    #[inline]
    fn clear_event_outputs(&mut self) {
        // Events still go through context for now
    }
}

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

        // Count stream inputs/outputs for IO struct sizing
        let num_stream_inputs = input_types
            .iter()
            .filter(|&&t| t == EndpointType::Stream)
            .count();
        let num_stream_outputs = output_types
            .iter()
            .filter(|&&t| t == EndpointType::Stream)
            .count();

        let node_key = self.nodes.insert(NodeData {
            processor: Box::new(node),
            inputs: inputs.clone(),
            outputs: outputs.clone(),
            input_types,
            output_types,
            has_event_inputs,
            num_stream_inputs,
            num_stream_outputs,
            create_io_fn: T::CREATE_IO_FN,
        });

        for &value_key in inputs.iter().chain(outputs.iter()) {
            self.value_to_node.insert(value_key, node_key);
        }

        self.topology_dirty = true;

        T::create_endpoints(node_key, inputs, outputs)
    }

    pub fn add_audio_input(&mut self) -> (<AudioInput as ProcessingNode>::Endpoints, ValueInput) {
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

    pub fn insert_value_input(
        &mut self,
        input: ValueInput,
        initial_value: f32,
    ) -> Option<ValueKey> {
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
                self.connections.entry(from).unwrap().or_default().push(to);
                self.topology_dirty = true;
            }
        }
    }

    pub fn disconnect<O, I>(&mut self, from: O, to: I) -> bool
    where
        O: Output,
        I: Into<InputEndpoint>,
    {
        let from_key = from.key();
        let to_key = to.into().key();

        let mut removed = false;

        if let Some(targets) = self.connections.get_mut(from_key) {
            let original_len = targets.len();
            targets.retain(|key| *key != to_key);
            if targets.len() != original_len {
                removed = true;
                if targets.is_empty() {
                    self.connections.remove(from_key);
                }
            }
        }

        if removed {
            self.topology_dirty = true;
        }

        removed
    }

    pub fn disconnect_all_from<O>(&mut self, from: O) -> bool
    where
        O: Output,
    {
        let from_key = from.key();
        if let Some(targets) = self.connections.remove(from_key) {
            if !targets.is_empty() {
                self.topology_dirty = true;
                return true;
            }
        }
        false
    }

    pub fn remove_node(&mut self, node_key: NodeKey) -> bool {
        let Some(node) = self.nodes.remove(node_key) else {
            return false;
        };

        let input_keys: Vec<ValueKey> = node.inputs.iter().copied().collect();
        let output_keys: Vec<ValueKey> = node.outputs.iter().copied().collect();

        self.node_order.retain(|&key| key != node_key);

        for &output_key in &output_keys {
            self.connections.remove(output_key);
        }

        for &input_key in &input_keys {
            self.remove_incoming_edges(input_key);
        }

        for &key in input_keys.iter().chain(output_keys.iter()) {
            self.remove_active_ramp(key);
            self.endpoints.remove(key);
            self.endpoint_types.remove(key);
            self.endpoint_descriptors.remove(key);
            self.value_to_node.remove(key);
        }

        self.topology_dirty = true;

        true
    }

    fn remove_incoming_edges(&mut self, target: ValueKey) {
        let source_keys: Vec<ValueKey> = self.connections.keys().collect();
        let mut empty_sources = Vec::new();

        for key in source_keys {
            if let Some(targets) = self.connections.get_mut(key) {
                targets.retain(|value| *value != target);
                if targets.is_empty() {
                    empty_sources.push(key);
                }
            }
        }

        for key in empty_sources {
            self.connections.remove(key);
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
            num_stream_inputs: 1,
            num_stream_outputs: 1,
            create_io_fn: FunctionNode::CREATE_IO_FN,
        });

        for &value_key in input_keys.iter().chain(output_keys.iter()) {
            self.value_to_node.insert(value_key, node_key);
        }

        self.topology_dirty = true;

        let output = StreamOutput::new(output_key);

        self.connect(from, InputEndpoint::new(input_key));

        output
    }

    pub fn combine<O1, O2>(&mut self, from1: O1, from2: O2, f: fn(f32, f32) -> f32) -> StreamOutput
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
            num_stream_inputs: 1,
            num_stream_outputs: 1,
            create_io_fn: BinaryFunctionNode::CREATE_IO_FN,
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
                    // Populate context arrays with input data
                    let mut input_values: [f32; MAX_NODE_ENDPOINTS] = [0.0; MAX_NODE_ENDPOINTS];
                    let mut value_inputs: [Option<&ValueData>; MAX_NODE_ENDPOINTS] =
                        [None; MAX_NODE_ENDPOINTS];
                    let mut event_inputs: [&[EventInstance]; MAX_NODE_ENDPOINTS] =
                        [&[]; MAX_NODE_ENDPOINTS];

                    let num_inputs = node.inputs.len();
                    let mut stream_input_idx = 0;

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
                                let value = endpoint_state
                                    .and_then(EndpointState::as_scalar)
                                    .unwrap_or(0.0);
                                input_values[idx] = value;
                                // Write to node via accessor method
                                node.processor.set_stream_input(stream_input_idx, value);
                                stream_input_idx += 1;
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

                    // Process (no IO struct - CMajor-style direct field access)
                    node.processor.process(self.sample_rate, &mut context);

                    // Route ALL stream outputs via accessor methods
                    let mut stream_output_idx = 0;
                    for (output_endpoint_idx, &output_type) in node.output_types.iter().enumerate()
                    {
                        if output_type == EndpointType::Stream {
                            // Read from node via accessor method
                            if let Some(output_value) = node.processor.get_stream_output(stream_output_idx) {
                                if let Some(&output_key) = node.outputs.get(output_endpoint_idx) {
                                    // Write to output endpoint
                                    if let Some(state) = self.endpoints.get_mut(output_key) {
                                        state.set_scalar(output_value);
                                    }

                                    // Copy to all connected inputs
                                    if let Some(connections) = self.connections.get(output_key) {
                                        for &target_input in connections {
                                            if let Some(target_state) =
                                                self.endpoints.get_mut(target_input)
                                            {
                                                target_state.set_scalar(output_value);
                                            }
                                        }
                                    }
                                }
                            }
                            stream_output_idx += 1;
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
                                            let _ =
                                                event_state.queue_mut().push(pending.event.clone());
                                        }
                                    }

                                    if let Some(targets) = self.connections.get(event_output_key) {
                                        for &target_input in targets {
                                            if let Some(target_state) =
                                                self.endpoints.get_mut(target_input)
                                            {
                                                if let Some(event_state) =
                                                    target_state.as_event_mut()
                                                {
                                                    let _ = event_state
                                                        .queue_mut()
                                                        .push(pending.event.clone());
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
        // Build adjacency map: node -> list of nodes that depend on it
        let adjacency = self.build_node_adjacency();

        // Create closures for the generic topological_sort function
        let nodes: Vec<NodeKey> = self.nodes.keys().collect();

        let get_dependencies = |node: &NodeKey| -> Vec<NodeKey> {
            // For topological sort, we need predecessors (dependencies)
            // The adjacency map has successors, so we need to build the reverse
            let mut deps = Vec::new();
            for (from, tos) in &adjacency {
                if tos.contains(node) {
                    deps.push(*from);
                }
            }
            deps
        };

        let allows_feedback = |node: &NodeKey| -> bool {
            self.nodes
                .get(*node)
                .map(|data| data.processor.allows_feedback())
                .unwrap_or(false)
        };

        // Call the generic topological sort
        super::topology::topological_sort(nodes, get_dependencies, allows_feedback)
            .map_err(|e| match e {
                super::topology::TopologyError::CycleDetected { path } => {
                    GraphError::CycleDetected(path)
                }
            })
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

    /// Ensure the node execution order is up to date.
    /// Called internally before processing and before converting to StaticGraph.
    pub fn update_topology(&mut self) -> Result<(), GraphError> {
        if self.topology_dirty {
            self.node_order = self.topological_sort()?;
            self.topology_dirty = false;
        }
        Ok(())
    }

    /// Get the node execution order (for StaticGraph conversion).
    /// Returns None if topology hasn't been computed yet.
    pub fn get_node_order(&self) -> &[NodeKey] {
        &self.node_order
    }

    /// Access to value_to_node mapping (for StaticGraph conversion)
    pub fn value_to_node(&self) -> &slotmap::SecondaryMap<ValueKey, NodeKey> {
        &self.value_to_node
    }
}
