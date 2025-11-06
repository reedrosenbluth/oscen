//! Memory layout computation for JIT-compiled graphs
//!
//! This module computes the memory layout for all node state, IO buffers,
//! and parameters, ensuring proper alignment and computing offsets for
//! efficient access during JIT code generation.

use crate::graph::{Graph, NodeKey};
use std::collections::HashMap;
use std::mem::{align_of, size_of};

/// Computed memory layout for a graph
///
/// Contains the offsets and sizes of all memory regions needed for
/// JIT-compiled graph execution.
#[derive(Debug, Clone)]
pub struct MemoryLayout {
    /// Offsets for each node's data
    pub node_offsets: HashMap<NodeKey, NodeOffsets>,

    /// Total size of state buffer
    pub state_size: usize,

    /// Total size of IO buffer
    pub io_size: usize,

    /// Total size of parameters buffer
    pub params_size: usize,

    /// Offset to final output value in IO buffer
    pub final_output_offset: usize,
}

/// Memory offsets for a single node
#[derive(Debug, Clone)]
pub struct NodeOffsets {
    /// Offset into state buffer (for persistent node state)
    pub state_offset: usize,

    /// Size of state data
    pub state_size: usize,

    /// Offset into IO buffer (for per-sample I/O)
    pub io_offset: usize,

    /// Size of IO struct
    pub io_size: usize,

    /// Offsets for each parameter (index matches parameter index)
    pub param_offsets: Vec<usize>,

    /// Field offsets within the IO struct
    /// Maps endpoint index to offset within the node's IO struct
    pub io_field_offsets: HashMap<usize, usize>,
}

impl MemoryLayout {
    /// Compute memory layout for a graph
    pub fn compute(graph: &Graph, topology: &[NodeKey]) -> Self {
        let mut node_offsets = HashMap::new();

        let mut state_offset = 0;
        let mut io_offset = 0;
        let mut params_offset = 0;

        // For each node in topology order, compute its offsets
        for &node_key in topology {
            let node_data = &graph.nodes[node_key];

            // Get size information from the node
            // For now, we'll use hardcoded sizes for known node types
            // In the future, this should be provided by a trait method
            let (state_size, io_size, param_count) = Self::get_node_sizes(node_data);

            // Align state offset to pointer alignment
            state_offset = Self::align_to(state_offset, align_of::<f32>());

            // Align IO offset
            io_offset = Self::align_to(io_offset, align_of::<f32>());

            // Compute parameter offsets
            let mut param_offsets = Vec::new();
            for _ in 0..param_count {
                params_offset = Self::align_to(params_offset, align_of::<f32>());
                param_offsets.push(params_offset);
                params_offset += size_of::<f32>();
            }

            // Compute IO field offsets within the IO struct
            // This maps endpoint indices to byte offsets
            let io_field_offsets = Self::compute_io_field_offsets(node_data);

            node_offsets.insert(
                node_key,
                NodeOffsets {
                    state_offset,
                    state_size,
                    io_offset,
                    io_size,
                    param_offsets,
                    io_field_offsets,
                },
            );

            state_offset += state_size;
            io_offset += io_size;
        }

        // The final output is the last node's output (for now, we'll use the first output field)
        let final_output_offset = if let Some(&last_node) = topology.last() {
            let last_offsets = &node_offsets[&last_node];
            last_offsets.io_offset // Assumes first field is output
        } else {
            0
        };

        MemoryLayout {
            node_offsets,
            state_size: state_offset,
            io_size: io_offset,
            params_size: params_offset,
            final_output_offset,
        }
    }

    /// Align offset to specified alignment
    fn align_to(offset: usize, align: usize) -> usize {
        (offset + align - 1) & !(align - 1)
    }

    /// Get size information for a node
    ///
    /// Returns (state_size, io_size, param_count)
    ///
    /// TODO: This should be provided by a trait method on the node type
    /// For now, we hardcode sizes for known types
    fn get_node_sizes(node_data: &crate::graph::NodeData) -> (usize, usize, usize) {
        // Get the debug representation to determine node type
        let debug_str = format!("{:?}", node_data.processor);

        // Match on node type name
        // This is a temporary solution - ideally nodes would provide this info via a trait
        if debug_str.contains("Gain") {
            // GainIO { input: f32, output: f32 } = 8 bytes
            // State: gain value = 4 bytes (stored in params, not state)
            (0, 8, 1) // state_size, io_size, param_count
        } else if debug_str.contains("Oscillator") {
            // OscillatorIO { output: f32 } = 4 bytes
            // State: phase (f32), phase_delta (f32) = 8 bytes
            (8, 4, 2) // state_size, io_size, param_count (frequency, amplitude)
        } else if debug_str.contains("TptFilter") {
            // TptFilterIO { input: f32, f_mod: f32, output: f32 } = 12 bytes
            // State: z[2], coefficients, etc. ≈ 32 bytes
            (32, 12, 2) // state_size, io_size, param_count (cutoff, q)
        } else {
            // Default sizes for unknown nodes
            (16, 8, 2)
        }
    }

    /// Compute field offsets within a node's IO struct
    ///
    /// Returns a map from endpoint index to byte offset within the IO struct
    ///
    /// TODO: This should be provided by the Node derive macro
    /// For now, we use a simple heuristic based on endpoint order
    fn compute_io_field_offsets(node_data: &crate::graph::NodeData) -> HashMap<usize, usize> {
        let mut offsets = HashMap::new();
        let mut current_offset = 0;

        // For each endpoint, assign an offset
        // Inputs come first, then outputs
        // Each stream endpoint is f32 (4 bytes)
        for (idx, endpoint_key) in node_data.inputs.iter().enumerate() {
            offsets.insert(idx, current_offset);
            current_offset += size_of::<f32>();
        }

        for (idx, endpoint_key) in node_data.outputs.iter().enumerate() {
            offsets.insert(node_data.inputs.len() + idx, current_offset);
            current_offset += size_of::<f32>();
        }

        offsets
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_align_to() {
        assert_eq!(MemoryLayout::align_to(0, 4), 0);
        assert_eq!(MemoryLayout::align_to(1, 4), 4);
        assert_eq!(MemoryLayout::align_to(4, 4), 4);
        assert_eq!(MemoryLayout::align_to(5, 4), 8);
        assert_eq!(MemoryLayout::align_to(7, 8), 8);
        assert_eq!(MemoryLayout::align_to(9, 8), 16);
    }
}
