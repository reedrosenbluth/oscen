mod audio_input;
mod graph_impl;
mod helpers;
mod traits;
pub mod types;

#[cfg(test)]
mod tests;

pub use audio_input::AudioInput;
pub use graph_impl::{Graph, GraphError, NodeData};
pub use traits::{ProcessingNode, SignalProcessor};
pub use types::{
    Connection, ConnectionBuilder, EndpointType, InputEndpoint, NodeKey, OutputEndpoint, ValueKey,
    MAX_CONNECTIONS_PER_OUTPUT, MAX_EVENTS, MAX_NODE_ENDPOINTS,
};
