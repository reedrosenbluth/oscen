/// Intermediate representation of a graph for JIT compilation
///
/// This IR captures the topology and data flow of the graph in a form
/// that can be translated to Cranelift IR for compilation.

use crate::graph::types::EndpointType;

/// Graph intermediate representation
#[derive(Debug, Clone)]
pub struct GraphIR {
    /// Nodes in the graph (matches SlotMap index order)
    pub nodes: Vec<NodeIR>,

    /// Connections between nodes
    pub connections: Vec<ConnectionIR>,

    /// Topologically sorted node execution order
    pub topology_order: Vec<usize>,

    /// Sample rate for the graph
    pub sample_rate: f32,
}

/// Node information for IR
#[derive(Debug, Clone)]
pub struct NodeIR {
    /// Index into the nodes array
    pub index: usize,

    /// SlotMap key for this node (for accessing the actual node)
    pub key_data: u64, // Store the key as u64

    /// Number of stream/value/event inputs
    pub num_stream_inputs: usize,
    pub num_value_inputs: usize,
    pub num_event_inputs: usize,

    /// Input endpoint keys (for gathering input values)
    pub stream_inputs: Vec<u64>, // ValueKey as u64
    pub value_inputs: Vec<u64>,
    pub event_inputs: Vec<u64>,

    /// Output endpoint keys
    pub stream_outputs: Vec<u64>,
    pub value_outputs: Vec<u64>,
    pub event_outputs: Vec<u64>,
}

/// Connection between two nodes
#[derive(Debug, Clone)]
pub struct ConnectionIR {
    /// Source node index
    pub src_node: usize,

    /// Source output index (which output of the node)
    pub src_output: usize,

    /// Destination node index
    pub dst_node: usize,

    /// Destination input index (which input of the node)
    pub dst_input: usize,

    /// Type of connection (stream, value, event)
    pub connection_type: EndpointType,
}

impl GraphIR {
    /// Validate the IR
    pub fn validate(&self) -> Result<(), String> {
        // Check topology order covers all nodes
        if self.topology_order.len() != self.nodes.len() {
            return Err("Topology order doesn't match node count".into());
        }

        // Check all node indices are valid
        for &idx in &self.topology_order {
            if idx >= self.nodes.len() {
                return Err(format!("Invalid node index in topology: {}", idx));
            }
        }

        // Check all connections reference valid nodes
        for conn in &self.connections {
            if conn.src_node >= self.nodes.len() {
                return Err(format!("Invalid source node: {}", conn.src_node));
            }
            if conn.dst_node >= self.nodes.len() {
                return Err(format!("Invalid destination node: {}", conn.dst_node));
            }
        }

        Ok(())
    }

    /// Get all connections where this node is the source
    pub fn connections_from(&self, node_idx: usize) -> Vec<&ConnectionIR> {
        self.connections
            .iter()
            .filter(|c| c.src_node == node_idx)
            .collect()
    }

    /// Get all connections where this node is the destination
    pub fn connections_to(&self, node_idx: usize) -> Vec<&ConnectionIR> {
        self.connections
            .iter()
            .filter(|c| c.dst_node == node_idx)
            .collect()
    }
}
