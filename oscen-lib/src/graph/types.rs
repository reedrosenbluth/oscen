use std::fmt;
use std::ops::Shr;
use std::sync::Arc;

use arrayvec::ArrayVec;
use slotmap::new_key_type;

pub const MAX_EVENTS: usize = 256;
pub const MAX_CONNECTIONS_PER_OUTPUT: usize = 1024;
pub const MAX_NODE_ENDPOINTS: usize = 16;

new_key_type! { pub struct NodeKey; }
new_key_type! { pub struct ValueKey; }

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum EndpointType {
    Stream,
    Value,
    Event,
}

pub trait ValueObject: Send + Sync + 'static + fmt::Debug {}

impl<T> ValueObject for T where T: Send + Sync + 'static + fmt::Debug {}

pub trait EventObject: Send + Sync + 'static + fmt::Debug {}

impl<T> EventObject for T where T: Send + Sync + 'static + fmt::Debug {}

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
    Stream(f32),
    Value(ValueData),
    Event(EventEndpointState),
}

impl EndpointState {
    pub fn stream(initial: f32) -> Self {
        Self::Stream(initial)
    }

    pub fn value(initial: f32) -> Self {
        Self::Value(ValueData::scalar(initial))
    }

    pub fn event() -> Self {
        Self::Event(EventEndpointState::new(MAX_EVENTS))
    }

    pub fn as_scalar(&self) -> Option<f32> {
        match self {
            Self::Stream(v) => Some(*v),
            Self::Value(data) => data.as_scalar(),
            Self::Event(_) => None,
        }
    }

    pub fn as_scalar_mut(&mut self) -> Option<&mut f32> {
        match self {
            Self::Stream(v) => Some(v),
            Self::Value(data) => data.as_scalar_mut(),
            Self::Event(_) => None,
        }
    }

    pub fn set_scalar(&mut self, value: f32) {
        match self {
            Self::Stream(slot) => *slot = value,
            Self::Value(data) => data.set_scalar(value),
            Self::Event(_) => {}
        }
    }

    pub fn as_event(&self) -> Option<&EventEndpointState> {
        match self {
            Self::Event(state) => Some(state),
            _ => None,
        }
    }

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

#[derive(Copy, Clone, Debug)]
pub struct OutputEndpoint {
    key: ValueKey,
}

impl OutputEndpoint {
    pub fn new(key: ValueKey) -> Self {
        Self { key }
    }

    pub fn to(self, input: InputEndpoint) -> ConnectionBuilder {
        self.shr(input)
    }

    pub fn key(&self) -> ValueKey {
        self.key
    }
}

pub struct Connection {
    pub(crate) from: OutputEndpoint,
    pub(crate) to: InputEndpoint,
}

pub struct ConnectionBuilder {
    pub(crate) from: OutputEndpoint,
    pub(crate) connections: ArrayVec<Connection, MAX_CONNECTIONS_PER_OUTPUT>,
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

impl From<ConnectionBuilder> for ArrayVec<Connection, MAX_CONNECTIONS_PER_OUTPUT> {
    fn from(builder: ConnectionBuilder) -> Self {
        builder.connections
    }
}
