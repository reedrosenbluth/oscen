use std::any::Any;
use std::fmt;
use std::ops::Shr;
use std::sync::Arc;

use arrayvec::ArrayVec;
use slotmap::new_key_type;

pub const MAX_EVENTS: usize = 256;
pub const MAX_CONNECTIONS_PER_OUTPUT: usize = 1024;
pub const MAX_NODE_ENDPOINTS: usize = 32;
pub const MAX_STREAM_CHANNELS: usize = 128;

new_key_type! { pub struct NodeKey; }
new_key_type! { pub struct ValueKey; }

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum EndpointType {
    Stream,
    Value,
    Event,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum EndpointDirection {
    Input,
    Output,
}

#[derive(Clone, Debug, Default)]
pub struct EndpointAnnotations {
    // Placeholder for future UI/validation metadata.
}

#[derive(Clone, Debug)]
pub struct EndpointDescriptor {
    pub name: &'static str,
    pub endpoint_type: EndpointType,
    pub direction: EndpointDirection,
    pub annotations: EndpointAnnotations,
}

impl EndpointDescriptor {
    pub const fn new(
        name: &'static str,
        endpoint_type: EndpointType,
        direction: EndpointDirection,
    ) -> Self {
        Self {
            name,
            endpoint_type,
            direction,
            annotations: EndpointAnnotations {},
        }
    }
}

pub trait ValueObject: Send + Sync + 'static + fmt::Debug {}

impl<T> ValueObject for T where T: Send + Sync + 'static + fmt::Debug {}

pub trait EventObject: Send + Sync + 'static + fmt::Debug {
    fn as_any(&self) -> &dyn Any;
}

impl<T> EventObject for T
where
    T: Send + Sync + 'static + fmt::Debug,
{
    fn as_any(&self) -> &dyn Any {
        self
    }
}

#[derive(Clone)]
pub enum ValueData {
    Scalar(f32),
    Object(Arc<dyn ValueObject>),
}

impl fmt::Debug for ValueData {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Scalar(v) => f.debug_tuple("Scalar").field(v).finish(),
            Self::Object(obj) => f.debug_tuple("Object").field(obj).finish(),
        }
    }
}

impl ValueData {
    pub fn scalar(value: f32) -> Self {
        Self::Scalar(value)
    }

    pub fn object<T>(value: T) -> Self
    where
        T: ValueObject,
    {
        Self::Object(Arc::new(value))
    }

    pub fn as_scalar(&self) -> Option<f32> {
        match self {
            Self::Scalar(v) => Some(*v),
            Self::Object(_) => None,
        }
    }

    pub fn as_scalar_mut(&mut self) -> Option<&mut f32> {
        match self {
            Self::Scalar(v) => Some(v),
            Self::Object(_) => None,
        }
    }

    pub fn as_object(&self) -> Option<&dyn ValueObject> {
        match self {
            Self::Scalar(_) => None,
            Self::Object(obj) => Some(obj.as_ref()),
        }
    }

    pub fn set_scalar(&mut self, value: f32) {
        if let Some(slot) = self.as_scalar_mut() {
            *slot = value;
        } else {
            *self = Self::Scalar(value);
        }
    }
}

#[derive(Clone)]
pub enum EventPayload {
    Scalar(f32),
    Object(Arc<dyn EventObject>),
}

impl fmt::Debug for EventPayload {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Scalar(v) => f.debug_tuple("Scalar").field(v).finish(),
            Self::Object(obj) => f.debug_tuple("Object").field(obj).finish(),
        }
    }
}

impl EventPayload {
    pub fn scalar(value: f32) -> Self {
        Self::Scalar(value)
    }

    pub fn object<T>(value: T) -> Self
    where
        T: EventObject,
    {
        Self::Object(Arc::new(value))
    }

    pub fn as_scalar(&self) -> Option<f32> {
        match self {
            Self::Scalar(v) => Some(*v),
            Self::Object(_) => None,
        }
    }

    pub fn as_object(&self) -> Option<&dyn EventObject> {
        match self {
            Self::Scalar(_) => None,
            Self::Object(obj) => Some(obj.as_ref()),
        }
    }
}

#[derive(Debug, Clone)]
pub struct EventEndpointState {
    queue: EventQueue,
}

impl EventEndpointState {
    pub fn new(max_events: usize) -> Self {
        Self {
            queue: EventQueue::new(max_events),
        }
    }

    pub fn queue(&self) -> &EventQueue {
        &self.queue
    }

    pub fn queue_mut(&mut self) -> &mut EventQueue {
        &mut self.queue
    }
}

#[derive(Debug)]
pub enum EndpointState {
    Stream(ArrayVec<f32, MAX_STREAM_CHANNELS>),
    Value(ValueData),
    Event(EventEndpointState),
}

impl EndpointState {
    pub fn stream(initial: f32) -> Self {
        let mut channels = ArrayVec::new();
        channels.push(initial);
        Self::Stream(channels)
    }

    pub fn value(initial: f32) -> Self {
        Self::Value(ValueData::scalar(initial))
    }

    pub fn event() -> Self {
        Self::Event(EventEndpointState::new(MAX_EVENTS))
    }

    #[inline]
    pub fn as_scalar(&self) -> Option<f32> {
        match self {
            Self::Stream(channels) => channels.first().copied(),
            Self::Value(data) => data.as_scalar(),
            Self::Event(_) => None,
        }
    }

    #[inline]
    pub fn as_scalar_mut(&mut self) -> Option<&mut f32> {
        match self {
            Self::Stream(channels) => channels.first_mut(),
            Self::Value(data) => data.as_scalar_mut(),
            Self::Event(_) => None,
        }
    }

    #[inline]
    pub fn set_scalar(&mut self, value: f32) {
        match self {
            Self::Stream(channels) => {
                channels.clear();
                channels.push(value);
            }
            Self::Value(data) => data.set_scalar(value),
            Self::Event(_) => {}
        }
    }

    #[inline]
    pub fn as_channels(&self) -> Option<&[f32]> {
        match self {
            Self::Stream(channels) => Some(channels.as_slice()),
            _ => None,
        }
    }

    #[inline]
    pub fn as_channels_mut(&mut self) -> Option<&mut ArrayVec<f32, MAX_STREAM_CHANNELS>> {
        match self {
            Self::Stream(channels) => Some(channels),
            _ => None,
        }
    }

    #[inline]
    pub fn set_channels(&mut self, values: &[f32]) {
        if let Self::Stream(channels) = self {
            channels.clear();
            channels.try_extend_from_slice(values).ok();
        }
    }

    #[inline]
    pub fn as_event(&self) -> Option<&EventEndpointState> {
        match self {
            Self::Event(state) => Some(state),
            _ => None,
        }
    }

    #[inline]
    pub fn as_event_mut(&mut self) -> Option<&mut EventEndpointState> {
        match self {
            Self::Event(state) => Some(state),
            _ => None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct EventInstance {
    pub frame_offset: u32,
    pub payload: EventPayload,
}

#[derive(Debug, Clone)]
pub struct EventQueue {
    events: Vec<EventInstance>,
    max_events: usize,
}

impl EventQueue {
    pub fn new(max_events: usize) -> Self {
        Self {
            events: Vec::with_capacity(max_events),
            max_events,
        }
    }

    pub fn clear(&mut self) {
        self.events.clear();
    }

    pub fn push(&mut self, event: EventInstance) -> bool {
        if self.events.len() < self.max_events {
            self.events.push(event);
            true
        } else {
            false
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = &EventInstance> {
        self.events.iter()
    }

    pub fn events(&self) -> &[EventInstance] {
        &self.events
    }

    pub fn events_mut(&mut self) -> &mut Vec<EventInstance> {
        &mut self.events
    }

    pub fn max_events(&self) -> usize {
        self.max_events
    }
}

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

// ============================================================================
// Typed Input Handles
// ============================================================================

#[derive(Copy, Clone, Debug)]
pub struct ValueInput {
    endpoint: InputEndpoint,
}

impl ValueInput {
    pub fn new(endpoint: InputEndpoint) -> Self {
        Self { endpoint }
    }

    pub fn endpoint(&self) -> InputEndpoint {
        self.endpoint
    }

    pub fn key(&self) -> ValueKey {
        self.endpoint.key()
    }
}

impl From<ValueInput> for ValueKey {
    fn from(handle: ValueInput) -> Self {
        handle.key()
    }
}

impl From<&ValueInput> for ValueKey {
    fn from(handle: &ValueInput) -> Self {
        handle.key()
    }
}

impl From<ValueInput> for InputEndpoint {
    fn from(handle: ValueInput) -> Self {
        handle.endpoint()
    }
}

impl From<&ValueInput> for InputEndpoint {
    fn from(handle: &ValueInput) -> Self {
        handle.endpoint()
    }
}

#[derive(Copy, Clone, Debug)]
pub struct StreamInput {
    endpoint: InputEndpoint,
}

impl StreamInput {
    pub fn new(endpoint: InputEndpoint) -> Self {
        Self { endpoint }
    }

    pub fn endpoint(&self) -> InputEndpoint {
        self.endpoint
    }

    pub fn key(&self) -> ValueKey {
        self.endpoint.key()
    }
}

impl From<StreamInput> for ValueKey {
    fn from(handle: StreamInput) -> Self {
        handle.key()
    }
}

impl From<&StreamInput> for ValueKey {
    fn from(handle: &StreamInput) -> Self {
        handle.key()
    }
}

impl From<StreamInput> for InputEndpoint {
    fn from(handle: StreamInput) -> Self {
        handle.endpoint()
    }
}

impl From<&StreamInput> for InputEndpoint {
    fn from(handle: &StreamInput) -> Self {
        handle.endpoint()
    }
}

#[derive(Copy, Clone, Debug)]
pub struct EventInput {
    endpoint: InputEndpoint,
}

impl EventInput {
    pub fn new(endpoint: InputEndpoint) -> Self {
        Self { endpoint }
    }

    pub fn endpoint(&self) -> InputEndpoint {
        self.endpoint
    }

    pub fn key(&self) -> ValueKey {
        self.endpoint.key()
    }
}

impl From<EventInput> for ValueKey {
    fn from(handle: EventInput) -> Self {
        handle.key()
    }
}

impl From<&EventInput> for ValueKey {
    fn from(handle: &EventInput) -> Self {
        handle.key()
    }
}

impl From<EventInput> for InputEndpoint {
    fn from(handle: EventInput) -> Self {
        handle.endpoint()
    }
}

impl From<&EventInput> for InputEndpoint {
    fn from(handle: &EventInput) -> Self {
        handle.endpoint()
    }
}

// ============================================================================
// Typed Output Handles
// ============================================================================

/// Trait for output endpoints - allows generic functions over all output types
pub trait Output {
    fn key(&self) -> ValueKey;
}

#[derive(Copy, Clone, Debug)]
pub struct StreamOutput {
    key: ValueKey,
}

impl StreamOutput {
    pub fn new(key: ValueKey) -> Self {
        Self { key }
    }

    pub fn key(&self) -> ValueKey {
        self.key
    }
}

impl Output for StreamOutput {
    fn key(&self) -> ValueKey {
        self.key
    }
}

#[derive(Copy, Clone, Debug)]
pub struct ValueOutput {
    key: ValueKey,
}

impl ValueOutput {
    pub fn new(key: ValueKey) -> Self {
        Self { key }
    }

    pub fn key(&self) -> ValueKey {
        self.key
    }
}

impl Output for ValueOutput {
    fn key(&self) -> ValueKey {
        self.key
    }
}

#[derive(Copy, Clone, Debug)]
pub struct EventOutput {
    key: ValueKey,
}

impl EventOutput {
    pub fn new(key: ValueKey) -> Self {
        Self { key }
    }

    pub fn key(&self) -> ValueKey {
        self.key
    }
}

impl Output for EventOutput {
    fn key(&self) -> ValueKey {
        self.key
    }
}

// ============================================================================
// ValueParam - Opaque parameter handle
// ============================================================================

/// An opaque handle to a value parameter that can be both updated and connected.
/// Created by `Graph::value_param()`.
#[derive(Copy, Clone, Debug)]
pub struct ValueParam {
    pub(crate) input: ValueInput,
    pub(crate) output: ValueOutput,
}

impl ValueParam {
    pub fn new(input: ValueInput, output: ValueOutput) -> Self {
        Self { input, output }
    }
}

impl Output for ValueParam {
    fn key(&self) -> ValueKey {
        self.output.key()
    }
}

// Allow ValueParam to be used where ValueKey is expected (for set_value, set_value_with_ramp, etc)
impl From<ValueParam> for ValueKey {
    fn from(param: ValueParam) -> Self {
        param.input.key()
    }
}

impl From<&ValueParam> for ValueKey {
    fn from(param: &ValueParam) -> Self {
        param.input.key()
    }
}

/// An opaque handle to an event parameter that can be both queued and connected.
/// Created by `Graph::event_param()`.
#[derive(Copy, Clone, Debug)]
pub struct EventParam {
    pub(crate) input: EventInput,
    pub(crate) output: EventOutput,
}

impl EventParam {
    pub fn new(input: EventInput, output: EventOutput) -> Self {
        Self { input, output }
    }
}

impl Output for EventParam {
    fn key(&self) -> ValueKey {
        self.output.key()
    }
}

// Allow EventParam to be used where InputEndpoint is expected (for queue_event)
impl From<EventParam> for InputEndpoint {
    fn from(param: EventParam) -> Self {
        param.input.into()
    }
}

impl From<&EventParam> for InputEndpoint {
    fn from(param: &EventParam) -> Self {
        param.input.into()
    }
}

// ============================================================================
// Connections
// ============================================================================

/// Internal representation of a connection (stores keys, not typed handles)
pub struct Connection {
    pub(crate) from: ValueKey,
    pub(crate) to: ValueKey,
}

pub struct ConnectionBuilder {
    pub(crate) from: ValueKey,
    pub(crate) connections: ArrayVec<Connection, MAX_CONNECTIONS_PER_OUTPUT>,
}

impl ConnectionBuilder {
    pub fn and<I>(mut self, to: I) -> Self
    where
        I: Into<InputEndpoint>,
    {
        self.connections.push(Connection {
            from: self.from,
            to: to.into().key(),
        });
        self
    }
}

impl From<ConnectionBuilder> for ArrayVec<Connection, MAX_CONNECTIONS_PER_OUTPUT> {
    fn from(builder: ConnectionBuilder) -> Self {
        builder.connections
    }
}

// ============================================================================
// Type-safe Stream connections (audio-rate)
// ============================================================================

impl Shr<StreamInput> for StreamOutput {
    type Output = ConnectionBuilder;

    fn shr(self, to: StreamInput) -> ConnectionBuilder {
        let mut builder = ConnectionBuilder {
            from: self.key(),
            connections: ArrayVec::new(),
        };
        builder.connections.push(Connection {
            from: self.key(),
            to: to.key(),
        });
        builder
    }
}

// ============================================================================
// Type-safe Value connections (control-rate)
// ============================================================================

impl Shr<ValueInput> for ValueOutput {
    type Output = ConnectionBuilder;

    fn shr(self, to: ValueInput) -> ConnectionBuilder {
        let mut builder = ConnectionBuilder {
            from: self.key(),
            connections: ArrayVec::new(),
        };
        builder.connections.push(Connection {
            from: self.key(),
            to: to.key(),
        });
        builder
    }
}

// ValueParam can be connected as if it were a ValueOutput
impl Shr<ValueInput> for ValueParam {
    type Output = ConnectionBuilder;

    fn shr(self, to: ValueInput) -> ConnectionBuilder {
        self.output >> to
    }
}

// ============================================================================
// Type-safe Event connections
// ============================================================================

impl Shr<EventInput> for EventOutput {
    type Output = ConnectionBuilder;

    fn shr(self, to: EventInput) -> ConnectionBuilder {
        let mut builder = ConnectionBuilder {
            from: self.key(),
            connections: ArrayVec::new(),
        };
        builder.connections.push(Connection {
            from: self.key(),
            to: to.key(),
        });
        builder
    }
}

// Allow routing event inputs to other event inputs
// This enables graph-level event inputs to be forwarded to node event inputs
impl Shr<EventInput> for EventInput {
    type Output = ConnectionBuilder;

    fn shr(self, to: EventInput) -> ConnectionBuilder {
        let mut builder = ConnectionBuilder {
            from: self.key(),
            connections: ArrayVec::new(),
        };
        builder.connections.push(Connection {
            from: self.key(),
            to: to.key(),
        });
        builder
    }
}

// Allow EventParam to connect to EventInput (uses the output of the passthrough node)
impl Shr<EventInput> for EventParam {
    type Output = ConnectionBuilder;

    fn shr(self, to: EventInput) -> ConnectionBuilder {
        self.output >> to
    }
}
