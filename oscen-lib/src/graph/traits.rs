use arrayvec::ArrayVec;

use super::types::NodeKey;
use super::types::{
    EndpointDescriptor, EventInstance, EventPayload, ValueData, ValueKey, ValueObject,
    MAX_NODE_ENDPOINTS, MAX_STREAM_CHANNELS,
};

#[derive(Copy, Clone)]
pub struct ValueRef<'a> {
    data: &'a ValueData,
}

impl<'a> ValueRef<'a> {
    pub(crate) fn new(data: &'a ValueData) -> Self {
        Self { data }
    }

    pub fn as_scalar(&self) -> Option<f32> {
        self.data.as_scalar()
    }

    pub fn as_object(&self) -> Option<&'a dyn ValueObject> {
        self.data.as_object()
    }

    pub fn data(&self) -> &'a ValueData {
        self.data
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct PendingEvent {
    pub output_index: usize,
    pub event: EventInstance,
}

pub struct ProcessingContext<'a> {
    stream_inputs: &'a [ArrayVec<f32, MAX_STREAM_CHANNELS>],
    value_inputs: &'a [Option<&'a ValueData>],
    event_inputs: &'a [&'a [EventInstance]],
    emitted_events: &'a mut Vec<PendingEvent>,
}

impl<'a> ProcessingContext<'a> {
    pub fn new(
        stream_inputs: &'a [ArrayVec<f32, MAX_STREAM_CHANNELS>],
        value_inputs: &'a [Option<&'a ValueData>],
        event_inputs: &'a [&'a [EventInstance]],
        emitted_events: &'a mut Vec<PendingEvent>,
    ) -> Self {
        Self {
            stream_inputs,
            value_inputs,
            event_inputs,
            emitted_events,
        }
    }

    #[inline]
    pub fn stream(&self, index: usize) -> f32 {
        self.stream_inputs
            .get(index)
            .and_then(|channels| channels.first())
            .copied()
            .unwrap_or(0.0)
    }

    #[inline]
    pub fn stream_channels(&self, index: usize) -> &[f32] {
        self.stream_inputs
            .get(index)
            .map(|channels| channels.as_slice())
            .unwrap_or(&[])
    }

    #[inline]
    pub fn value(&self, index: usize) -> Option<ValueRef<'a>> {
        self.value_inputs
            .get(index)
            .and_then(|opt| opt.map(ValueRef::new))
    }

    #[inline]
    pub fn value_scalar(&self, index: usize) -> f32 {
        self.value(index)
            .and_then(|value| value.as_scalar())
            .unwrap_or_else(|| self.stream(index))
    }

    #[inline]
    pub fn events(&self, index: usize) -> &[EventInstance] {
        self.event_inputs.get(index).copied().unwrap_or_default()
    }

    #[inline]
    pub fn emit_event(&mut self, output_index: usize, event: EventInstance) {
        self.emitted_events.push(PendingEvent {
            output_index,
            event,
        });
    }

    pub fn emit_timed_event(
        &mut self,
        output_index: usize,
        frame_offset: u32,
        payload: EventPayload,
    ) {
        self.emit_event(
            output_index,
            EventInstance {
                frame_offset,
                payload,
            },
        );
    }

    pub fn emit_scalar_event(&mut self, output_index: usize, frame_offset: u32, payload: f32) {
        self.emit_timed_event(output_index, frame_offset, EventPayload::scalar(payload));
    }

    pub fn emit_event_to_array(
        &mut self,
        output_index: usize,
        _array_index: usize,
        event: EventInstance,
    ) {
        // For ProcessingContext, we don't have direct array routing
        // Just emit the event normally - routing happens in the graph
        self.emit_event(output_index, event);
    }
}

/// Trait for event emission that works in both runtime and static graphs.
/// This provides a unified API for nodes to emit events regardless of graph type.
pub trait EventContext {
    /// Emit an event from an output endpoint
    fn emit_event(&mut self, output_index: usize, event: EventInstance);

    /// Emit a timed event with a frame offset and payload
    fn emit_timed_event(
        &mut self,
        output_index: usize,
        frame_offset: u32,
        payload: EventPayload,
    );

    /// Emit a scalar event (convenience method for f32 payloads)
    fn emit_scalar_event(&mut self, output_index: usize, frame_offset: u32, payload: f32);

    /// Emit an event to a specific array index (for array event outputs)
    /// This is used by nodes like VoiceAllocator that route to multiple destinations
    fn emit_event_to_array(
        &mut self,
        output_index: usize,
        array_index: usize,
        event: EventInstance,
    );
}

/// Implement EventContext for ProcessingContext (runtime graphs)
impl<'a> EventContext for ProcessingContext<'a> {
    #[inline]
    fn emit_event(&mut self, output_index: usize, event: EventInstance) {
        self.emit_event(output_index, event);
    }

    #[inline]
    fn emit_timed_event(
        &mut self,
        output_index: usize,
        frame_offset: u32,
        payload: EventPayload,
    ) {
        self.emit_timed_event(output_index, frame_offset, payload);
    }

    #[inline]
    fn emit_scalar_event(&mut self, output_index: usize, frame_offset: u32, payload: f32) {
        self.emit_scalar_event(output_index, frame_offset, payload);
    }

    #[inline]
    fn emit_event_to_array(
        &mut self,
        output_index: usize,
        _array_index: usize,
        event: EventInstance,
    ) {
        // For ProcessingContext, we don't have direct array routing
        // Just emit the event normally - routing happens in the graph
        self.emit_event(output_index, event);
    }
}

/// Trait for safe, type-erased access to IO struct fields.
/// This enables the graph to read outputs and write inputs without knowing the concrete IO struct type.
/// Implementations are generated by the #[derive(Node)] macro.
pub trait IOStructAccess: Send {
    /// Get the number of stream inputs
    fn num_stream_inputs(&self) -> usize;

    /// Get the number of stream outputs
    fn num_stream_outputs(&self) -> usize;

    /// Get the number of event outputs
    fn num_event_outputs(&self) -> usize;

    /// Set a stream input field (graph writes here before processing)
    fn set_stream_input(&mut self, index: usize, value: f32);

    /// Get a stream input field value (node reads during processing)
    fn get_stream_input(&self, index: usize) -> Option<f32>;

    /// Set a stream output field (node writes during processing)
    fn set_stream_output(&mut self, index: usize, value: f32);

    /// Get a stream output field value (graph reads after processing to route)
    fn get_stream_output(&self, index: usize) -> Option<f32>;

    /// Set multi-channel stream input (for arrays/multi-channel streams)
    fn set_stream_input_channels(&mut self, index: usize, channels: &[f32]) {
        // Default: set first channel only for backward compatibility
        if let Some(&first) = channels.first() {
            self.set_stream_input(index, first);
        }
    }

    /// Get multi-channel stream output (returns channels as slice)
    fn get_stream_output_channels(&self, _index: usize) -> &[f32] {
        // Default: return empty slice (no multi-channel support)
        &[]
    }

    /// Get event output instances (used after node processing to route events)
    fn get_event_output(&self, index: usize) -> &[EventInstance];

    /// Clear all event output buffers (called before each processing cycle)
    fn clear_event_outputs(&mut self);
}

/// Users implement this trait to define their DSP logic. Inputs are already
/// populated in the struct fields by the time process() is called.
pub trait SignalProcessor: Send + std::fmt::Debug {
    /// Called once when the node is added to a graph.
    fn init(&mut self, _sample_rate: f32) {}

    /// Process one sample of audio.
    ///
    /// All inputs are already populated in struct fields. Write outputs to
    /// output fields. No context object to deal with!
    ///
    /// Sample rate is stored in the node during init() or construction.
    /// For static graphs, this is called directly with zero overhead.
    /// For runtime graphs, NodeIO::read_inputs() is called first to populate inputs.
    fn process(&mut self);

    /// Whether this node can break feedback cycles (e.g., delay lines).
    #[inline]
    fn allows_feedback(&self) -> bool {
        false
    }

    /// Returns whether this node is currently active and producing meaningful output.
    /// Inactive nodes can be skipped during processing, with their outputs set to 0.0.
    #[inline]
    fn is_active(&self) -> bool {
        true
    }
}

/// Node IO trait - handles reading inputs from context and providing outputs.
/// This is auto-generated by the #[derive(Node)] macro.
pub trait NodeIO {
    /// Read all inputs from the processing context into struct fields.
    /// Called by runtime graphs before process().
    fn read_inputs<'a>(&mut self, context: &mut ProcessingContext<'a>);

    /// Get stream output value by index (for runtime graph routing).
    #[inline]
    fn get_stream_output(&self, _index: usize) -> Option<f32> {
        None
    }

    /// Set stream input value by index (for runtime graph routing).
    #[inline]
    fn set_stream_input(&mut self, _index: usize, _value: f32) {
        // Default: do nothing
    }

    /// Get multi-channel stream output (returns channels as slice)
    #[inline]
    fn get_stream_output_channels(&self, _index: usize) -> &[f32] {
        &[]
    }

    /// Set multi-channel stream input
    #[inline]
    fn set_stream_input_channels(&mut self, index: usize, channels: &[f32]) {
        // Default: set first channel only for backward compatibility
        if let Some(&first) = channels.first() {
            self.set_stream_input(index, first);
        }
    }

    /// Get value output by index (for runtime graph routing).
    #[inline]
    fn get_value_output(&self, _index: usize) -> Option<ValueData> {
        None
    }
}

/// Marker trait for nodes that can be processed in the runtime graph.
/// Automatically implemented for all types that are SignalProcessor + NodeIO.
pub trait DynNode: SignalProcessor + NodeIO {}

// Blanket implementation: any type that implements both traits gets this for free
impl<T: SignalProcessor + NodeIO> DynNode for T {}

/// Trait for nodes that route events to array outputs at runtime.
/// Inspired by CMajor's `voiceEventOut[index] <- event` pattern.
///
/// Nodes like VoiceAllocator implement this to provide runtime multiplexing:
/// incoming events are routed to different output indices based on runtime state
/// (e.g., voice allocation, round-robin, etc.).
pub trait ArrayEventOutput {
    /// Process an event from the given input and return which output index to route it to.
    /// Returns None if the event should not be routed.
    ///
    /// # Arguments
    /// * `input_index` - Which input endpoint the event arrived at (e.g., 0 for note_on, 1 for note_off)
    /// * `event` - The event to process
    fn route_event(&mut self, input_index: usize, event: &EventInstance) -> Option<usize>;
}

pub trait ProcessingNode: SignalProcessor + NodeIO {
    type Endpoints;

    const ENDPOINT_DESCRIPTORS: &'static [EndpointDescriptor] = &[];

    /// Factory function to create IO struct for this node type.
    /// Generated by #[derive(Node)] macro, returns node-specific IO struct.
    /// Default returns DynamicIO for helper nodes.
    const CREATE_IO_FN: fn() -> Box<dyn IOStructAccess> =
        || Box::new(crate::graph::graph_impl::DynamicIO::new(0, 0));

    fn create_endpoints(
        node_key: NodeKey,
        inputs: ArrayVec<ValueKey, MAX_NODE_ENDPOINTS>,
        outputs: ArrayVec<ValueKey, MAX_NODE_ENDPOINTS>,
    ) -> Self::Endpoints;

    /// Returns initial values for value inputs as (input_index, value) pairs.
    /// Called during node addition to initialize graph endpoints from constructor arguments.
    fn default_values(&self) -> Vec<(usize, f32)> {
        vec![]
    }
}
