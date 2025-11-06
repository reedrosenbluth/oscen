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
/// This is a generic trampoline - in the future we could generate specialized
/// trampolines per node type for better performance.
///
/// SAFETY: state must be a valid pointer to GraphState with valid node references
pub extern "C" fn process_node_trampoline(
    node_index: usize,
    sample_rate: f32,
    context_ptr: *mut (),
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
        if let Some(node_data) = nodes.get_mut(node_key) {
            // Cast context
            let context: &mut ProcessingContext = &mut *(context_ptr as *mut ProcessingContext);

            // Call the actual process method
            node_data.processor.process(sample_rate, context)
        } else {
            // Node not found - return 0.0
            0.0
        }
    }
}

/// Helper function to create a ProcessingContext from raw data
///
/// This is called by JIT code to prepare the context before calling process.
pub unsafe fn create_processing_context<'a>(
    state: &'a mut GraphState,
    node_idx: usize,
    endpoints: &'a SlotMap<ValueKey, EndpointState>,
) -> ProcessingContext<'a> {
    use slotmap::Key;

    // Get input range for this node
    let input_start = *state.input_offsets.add(node_idx);
    let input_end = *state.input_offsets.add(node_idx + 1);
    let num_inputs = input_end - input_start;

    // Gather inputs from endpoints
    let input_keys = std::slice::from_raw_parts(
        state.endpoint_keys.add(input_start),
        num_inputs,
    );

    // Prepare input arrays
    let input_values = std::slice::from_raw_parts_mut(
        state.temp_input_values,
        num_inputs,
    );

    // TODO: Fill input_values from endpoints using input_keys
    // For now, just zero them
    for i in 0..num_inputs {
        input_values[i] = 0.0;
    }

    // Create context (simplified for now)
    let events_buffer: &mut Vec<super::super::traits::PendingEvent> = &mut *state.temp_events_buffer;

    ProcessingContext::new(
        input_values,
        &[], // value_inputs - TODO
        &[], // event_inputs - TODO
        events_buffer,
    )
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
