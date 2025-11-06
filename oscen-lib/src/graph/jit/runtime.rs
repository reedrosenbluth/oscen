/// Runtime support for JIT-compiled graph execution
///
/// This module provides the bridge between JIT-compiled machine code
/// and the Rust node implementations.

use super::super::traits::{ProcessingContext, SignalProcessor};
use super::super::types::{EndpointState, EndpointType, ValueKey};
use super::super::graph_impl::NodeData;
use super::ir::GraphIR;
use slotmap::SlotMap;
use std::mem;

/// C-compatible function pointer type for node processing
///
/// This is what the JIT-compiled code will call.
/// Signature: fn(node_index: usize, sample_rate: f32, context: *mut (), state: *mut ()) -> f32
///
/// We pass node_index instead of a node pointer to avoid the fat pointer issue.
/// The state pointer allows the trampoline to access the actual node.
pub type ProcessFnPtr = extern "C" fn(usize, f32, *mut (), *mut GraphState) -> f32;

/// Runtime state passed to JIT-compiled code
///
/// This struct must be #[repr(C)] for stable layout that Cranelift can work with.
/// All pointers point to data owned by the Graph, which must outlive the compiled code.
#[repr(C)]
pub struct GraphState {
    /// Pointer to the nodes SlotMap
    /// This allows safe access to nodes by index
    pub nodes_slotmap: *mut SlotMap<super::super::types::NodeKey, NodeData>,

    /// Array of NodeKeys in topology order
    /// Length: node_count
    pub node_keys: *const u64,

    /// Pointer to endpoint SlotMap data
    pub endpoints: *mut SlotMap<ValueKey, EndpointState>,

    /// Array of endpoint keys for looking up values
    /// This is a flattened array: [node0_inputs..., node1_inputs..., ...]
    pub endpoint_keys: *const u64,

    /// Offsets into endpoint_keys for each node's inputs
    /// Length: node_count + 1 (last entry is total length)
    pub input_offsets: *const usize,

    /// Offsets into endpoint_keys for each node's outputs
    /// Length: node_count + 1
    pub output_offsets: *const usize,

    /// Sample rate
    pub sample_rate: f32,

    /// Number of nodes
    pub node_count: usize,

    /// Temporary buffers for ProcessingContext (to avoid allocation per node)
    /// These must be sized appropriately before use
    pub temp_input_values: *mut f32,
    pub temp_value_inputs: *mut *const (),
    pub temp_event_inputs: *mut *const (),
    pub temp_events_buffer: *mut Vec<super::super::traits::PendingEvent>,
}

/// Builder for GraphState
///
/// This manages the memory and lifetime of all data needed for JIT execution.
pub struct GraphStateBuilder {
    /// Node keys in topology order
    node_keys: Vec<u64>,

    /// Flattened endpoint keys
    endpoint_keys: Vec<u64>,

    /// Input offsets for each node
    input_offsets: Vec<usize>,

    /// Output offsets for each node
    output_offsets: Vec<usize>,

    /// Temporary buffers
    temp_input_values: Vec<f32>,
    temp_value_inputs: Vec<*const ()>,
    temp_event_inputs: Vec<*const ()>,
    temp_events_buffer: Vec<super::super::traits::PendingEvent>,

    /// Sample rate
    sample_rate: f32,
}

impl GraphStateBuilder {
    /// Create a new builder for the given graph IR
    pub fn new(ir: &GraphIR, _nodes: &mut SlotMap<super::super::types::NodeKey, NodeData>) -> Self {
        let node_count = ir.nodes.len();

        let mut node_keys = Vec::with_capacity(node_count);
        let mut endpoint_keys = Vec::new();
        let mut input_offsets = Vec::with_capacity(node_count + 1);
        let mut output_offsets = Vec::with_capacity(node_count + 1);

        // Collect all inputs first, then all outputs (keeps them contiguous)
        let mut all_node_inputs = Vec::new();
        let mut all_node_outputs = Vec::new();

        input_offsets.push(0);
        output_offsets.push(0);

        // For each node in the IR
        for node_ir in &ir.nodes {
            // Store the node key
            node_keys.push(node_ir.key_data);

            // Collect this node's input keys
            let mut node_inputs = Vec::new();
            for &input_key_data in &node_ir.stream_inputs {
                node_inputs.push(input_key_data);
            }
            for &input_key_data in &node_ir.value_inputs {
                node_inputs.push(input_key_data);
            }
            for &input_key_data in &node_ir.event_inputs {
                node_inputs.push(input_key_data);
            }

            // Collect this node's output keys
            let mut node_outputs = Vec::new();
            for &output_key_data in &node_ir.stream_outputs {
                node_outputs.push(output_key_data);
            }
            for &output_key_data in &node_ir.event_outputs {
                node_outputs.push(output_key_data);
            }

            all_node_inputs.extend(&node_inputs);
            all_node_outputs.extend(&node_outputs);

            input_offsets.push(all_node_inputs.len());
            output_offsets.push(all_node_outputs.len());
        }

        // Build final endpoint_keys: [all inputs..., all outputs...]
        endpoint_keys.extend(&all_node_inputs);
        let outputs_start = endpoint_keys.len();
        endpoint_keys.extend(&all_node_outputs);

        // Adjust output_offsets to account for inputs_start
        for offset in &mut output_offsets {
            *offset += outputs_start;
        }

        // Allocate temporary buffers (sized for worst case)
        let max_inputs = 32; // MAX_NODE_ENDPOINTS
        let temp_input_values = vec![0.0f32; max_inputs];
        let temp_value_inputs = vec![std::ptr::null(); max_inputs];
        let temp_event_inputs = vec![std::ptr::null(); max_inputs];
        let temp_events_buffer = Vec::with_capacity(64);

        Self {
            node_keys,
            endpoint_keys,
            input_offsets,
            output_offsets,
            temp_input_values,
            temp_value_inputs,
            temp_event_inputs,
            temp_events_buffer,
            sample_rate: ir.sample_rate,
        }
    }

    /// Build the GraphState that can be passed to JIT code
    ///
    /// The returned GraphState contains pointers into this builder's data,
    /// so the builder must outlive any use of the GraphState.
    pub fn build(
        &mut self,
        nodes: &mut SlotMap<super::super::types::NodeKey, NodeData>,
        endpoints: &mut SlotMap<ValueKey, EndpointState>,
    ) -> GraphState {
        GraphState {
            nodes_slotmap: nodes as *mut _,
            node_keys: self.node_keys.as_ptr(),
            endpoints: endpoints as *mut _,
            endpoint_keys: self.endpoint_keys.as_ptr(),
            input_offsets: self.input_offsets.as_ptr(),
            output_offsets: self.output_offsets.as_ptr(),
            sample_rate: self.sample_rate,
            node_count: self.node_keys.len(),
            temp_input_values: self.temp_input_values.as_mut_ptr(),
            temp_value_inputs: self.temp_value_inputs.as_mut_ptr(),
            temp_event_inputs: self.temp_event_inputs.as_mut_ptr(),
            temp_events_buffer: &mut self.temp_events_buffer as *mut _,
        }
    }
}

/// Get the function pointer to the trampoline for registration with Cranelift
pub fn get_trampoline_ptr() -> ProcessFnPtr {
    process_node_trampoline
}

/// Trampoline function that JIT code calls to invoke a node's process method
///
/// This is a generic trampoline that:
/// 1. Builds a ProcessingContext from endpoints
/// 2. Calls the node's process() method
/// 3. Returns the result
///
/// SAFETY: state must be a valid pointer to GraphState with valid node references
pub extern "C" fn process_node_trampoline(
    node_index: usize,
    sample_rate: f32,
    _context_ptr: *mut (), // Unused - we build context here
    state_ptr: *mut GraphState,
) -> f32 {
    unsafe {
        println!("[TRAMPOLINE] Processing node {}", node_index);

        // Get the GraphState
        let state: &mut GraphState = &mut *state_ptr;

        // Reconstruct the NodeKey from the stored u64
        use slotmap::Key;
        let node_key_data = *state.node_keys.add(node_index);
        let node_key = super::super::types::NodeKey::from(slotmap::KeyData::from_ffi(node_key_data));

        // Access the node from the SlotMap
        let nodes: &mut SlotMap<super::super::types::NodeKey, NodeData> = &mut *state.nodes_slotmap;
        let endpoints: &mut SlotMap<ValueKey, EndpointState> = &mut *state.endpoints;

        if let Some(node_data) = nodes.get_mut(node_key) {
            // Build ProcessingContext

            // Get input range for this node
            let input_start = *state.input_offsets.add(node_index);
            let input_end = *state.input_offsets.add(node_index + 1);
            let num_inputs = input_end - input_start;

            // Get all input endpoint keys
            let input_keys = if num_inputs > 0 {
                std::slice::from_raw_parts(
                    state.endpoint_keys.add(input_start),
                    num_inputs,
                )
            } else {
                &[]
            };

            // Build input arrays using OVERALL indices (not type-specific)
            // All three arrays (stream_inputs, value_inputs, event_inputs) use the same indices
            let stream_input_values = std::slice::from_raw_parts_mut(
                state.temp_input_values,
                if num_inputs > 0 { num_inputs } else { 1 },
            );

            // Initialize all to 0.0
            for i in 0..num_inputs {
                stream_input_values[i] = 0.0;
            }

            // Fill values for each input based on its type
            for (i, &input_type) in node_data.input_types.iter().enumerate() {
                if i >= input_keys.len() { break; }

                let value_key = ValueKey::from(slotmap::KeyData::from_ffi(input_keys[i]));
                if let Some(endpoint_state) = endpoints.get(value_key) {
                    // Fill stream_input_values for all types
                    stream_input_values[i] = endpoint_state.as_scalar().unwrap_or(0.0);
                    println!("[TRAMPOLINE] Node {} input[{}] = {}", node_index, i, stream_input_values[i]);
                }
            }


            // Build value_inputs array using OVERALL indices
            // The value_inputs array must be the same size as stream_inputs and use the same indices!
            let value_input_values = std::slice::from_raw_parts_mut(
                state.temp_value_inputs as *mut Option<&super::super::types::ValueData>,
                if num_inputs > 0 { num_inputs } else { 1 },
            );

            // Initialize all to None
            for i in 0..num_inputs {
                value_input_values[i] = None;
            }

            // Fill value references for Value-type inputs
            for (i, &input_type) in node_data.input_types.iter().enumerate() {
                if i >= input_keys.len() { break; }

                if input_type == super::super::types::EndpointType::Value {
                    let value_key = ValueKey::from(slotmap::KeyData::from_ffi(input_keys[i]));
                    if let Some(endpoint_state) = endpoints.get(value_key) {
                        if let super::super::types::EndpointState::Value(value_data) = endpoint_state {
                            value_input_values[i] = Some(value_data);
                        }
                    }
                }
            }

            // Create context with overall-indexed arrays
            let events_buffer: &mut Vec<super::super::traits::PendingEvent> = &mut *state.temp_events_buffer;

            let stream_slice = if num_inputs > 0 {
                &stream_input_values[..num_inputs]
            } else {
                &[]
            };

            let value_slice = if num_inputs > 0 {
                &value_input_values[..num_inputs]
            } else {
                &[]
            };

            let mut context = ProcessingContext::new(
                stream_slice,
                value_slice,
                &[], // event_inputs - TODO
                events_buffer,
            );

            // Call the actual process method
            let result = node_data.processor.process(sample_rate, &mut context);
            println!("[TRAMPOLINE] Node {} returned {}", node_index, result);
            result
        } else {
            // Node not found - return 0.0
            println!("[TRAMPOLINE] Node {} not found!", node_index);
            0.0
        }
    }
}

/// Helper function to write a node's output back to its endpoint
///
/// This is called by JIT code after processing to update the endpoint
/// so downstream nodes can read the value.
///
/// SAFETY: state must be valid and node_index must be in range
pub unsafe extern "C" fn write_node_output(
    state: *mut GraphState,
    node_index: usize,
    output_value: f32,
) {
    use slotmap::Key;

    println!("[WRITE_OUTPUT] Node {} writing {}", node_index, output_value);

    let state = &mut *state;
    let endpoints: &mut SlotMap<ValueKey, EndpointState> = &mut *state.endpoints;

    // Get output range for this node
    let output_start = *state.output_offsets.add(node_index);
    let output_end = *state.output_offsets.add(node_index + 1);

    println!("[WRITE_OUTPUT] output_start={}, output_end={}", output_start, output_end);

    if output_end > output_start {
        // Write to the first output endpoint (primary output)
        let output_key_data = *state.endpoint_keys.add(output_start);
        let output_key = ValueKey::from(slotmap::KeyData::from_ffi(output_key_data));

        if let Some(endpoint_state) = endpoints.get_mut(output_key) {
            endpoint_state.set_scalar(output_value);
            println!("[WRITE_OUTPUT] Successfully wrote to endpoint");
        } else {
            println!("[WRITE_OUTPUT] Endpoint not found!");
        }
    } else {
        println!("[WRITE_OUTPUT] No outputs to write");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_graph_state_layout() {
        // Verify GraphState is repr(C) and has stable layout
        // 12 pointer fields (8 bytes each on 64-bit) + 1 f32 (4 bytes) + 1 usize (8 bytes) = 96 + 4 + 8 = 108... wait that's not right
        // Let me count: nodes_slotmap, node_keys, endpoints, endpoint_keys, input_offsets, output_offsets (6 pointers)
        // temp_input_values, temp_value_inputs, temp_event_inputs, temp_events_buffer (4 pointers)
        // = 10 pointers * 8 = 80 bytes
        // + sample_rate (f32 = 4 bytes) + node_count (usize = 8 bytes) = 80 + 4 + 8 = 92 bytes
        // But struct padding might add 4 bytes to align to 8-byte boundary
        // Actual size is 96 bytes
        assert_eq!(mem::size_of::<GraphState>(), 96);
    }
}
