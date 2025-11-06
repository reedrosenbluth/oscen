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

        input_offsets.push(0);
        output_offsets.push(0);

        // For each node in the IR
        for node_ir in &ir.nodes {
            // Store the node key
            node_keys.push(node_ir.key_data);

            // Collect input endpoint keys
            for &input_key_data in &node_ir.stream_inputs {
                endpoint_keys.push(input_key_data);
            }
            for &input_key_data in &node_ir.value_inputs {
                endpoint_keys.push(input_key_data);
            }
            for &input_key_data in &node_ir.event_inputs {
                endpoint_keys.push(input_key_data);
            }
            input_offsets.push(endpoint_keys.len());

            // Collect output endpoint keys
            for &output_key_data in &node_ir.stream_outputs {
                endpoint_keys.push(output_key_data);
            }
            for &output_key_data in &node_ir.event_outputs {
                endpoint_keys.push(output_key_data);
            }
            output_offsets.push(endpoint_keys.len());
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

            // Fill stream inputs (first num_stream_inputs inputs)
            let num_stream = node_data.input_types.iter()
                .filter(|&&t| t == super::super::types::EndpointType::Stream)
                .count();

            let stream_input_values = std::slice::from_raw_parts_mut(
                state.temp_input_values,
                if num_stream > 0 { num_stream } else { 1 },
            );

            for i in 0..num_stream.min(input_keys.len()) {
                let value_key = ValueKey::from(slotmap::KeyData::from_ffi(input_keys[i]));
                if let Some(endpoint_state) = endpoints.get(value_key) {
                    stream_input_values[i] = endpoint_state.as_scalar().unwrap_or(0.0);
                } else {
                    stream_input_values[i] = 0.0;
                }
            }


            // Create context
            let events_buffer: &mut Vec<super::super::traits::PendingEvent> = &mut *state.temp_events_buffer;

            let stream_slice = if num_stream > 0 {
                &stream_input_values[..num_stream]
            } else {
                &[]
            };

            // Build value inputs array properly
            // Count value inputs
            let num_value = node_data.input_types.iter()
                .filter(|&&t| t == super::super::types::EndpointType::Value)
                .count();

            // Create temporary Vec for value inputs (needs to live through process() call)
            let mut value_input_options: Vec<Option<&super::super::types::ValueData>> = Vec::with_capacity(num_value.max(1));

            // Fill value inputs by iterating through all inputs in order
            for (i, &input_type) in node_data.input_types.iter().enumerate() {
                if input_type == super::super::types::EndpointType::Value && i < input_keys.len() {
                    let value_key = ValueKey::from(slotmap::KeyData::from_ffi(input_keys[i]));
                    if let Some(endpoint_state) = endpoints.get(value_key) {
                        // Get reference to ValueData
                        match endpoint_state {
                            super::super::types::EndpointState::Value(value_data) => {
                                value_input_options.push(Some(value_data));
                            }
                            _ => {
                                value_input_options.push(None);
                            }
                        }
                    } else {
                        value_input_options.push(None);
                    }
                }
            }

            let value_slice = if num_value > 0 {
                &value_input_options[..]
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
            node_data.processor.process(sample_rate, &mut context)
        } else {
            // Node not found - return 0.0
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

    let state = &mut *state;
    let endpoints: &mut SlotMap<ValueKey, EndpointState> = &mut *state.endpoints;

    // Get output range for this node
    let output_start = *state.output_offsets.add(node_index);
    let output_end = *state.output_offsets.add(node_index + 1);

    if output_end > output_start {
        // Write to the first output endpoint (primary output)
        let output_key_data = *state.endpoint_keys.add(output_start);
        let output_key = ValueKey::from(slotmap::KeyData::from_ffi(output_key_data));

        if let Some(endpoint_state) = endpoints.get_mut(output_key) {
            endpoint_state.set_scalar(output_value);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_graph_state_layout() {
        // Verify GraphState is repr(C) and has stable layout
        assert_eq!(mem::size_of::<GraphState>(), mem::size_of::<usize>() * 10 + 8);
    }
}
