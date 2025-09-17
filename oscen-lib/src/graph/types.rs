use std::ops::Shr;

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
