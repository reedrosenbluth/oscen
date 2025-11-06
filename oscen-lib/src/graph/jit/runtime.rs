/// Runtime support for JIT-compiled graph execution
///
/// This module provides the bridge between JIT-compiled machine code
/// and the Rust node implementations.

use super::super::traits::ProcessingContext;
use super::super::types::{EndpointState, EventInstance, ValueKey};
use super::super::graph_impl::NodeData;
use super::ir::GraphIR;
use slotmap::{Key, SlotMap};

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

    /// Connections: maps output endpoint index to connected input endpoints
    /// For each output endpoint, stores offset into connections_data
    /// Length: total number of output endpoints + 1
    pub connections_offsets: *const usize,

    /// Connection data: flattened array of connected input endpoint keys (u64)
    pub connections_data: *const u64,

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

    /// Connection offsets (maps output endpoint index to offset in connections_data)
    connections_offsets: Vec<usize>,

    /// Connected input endpoint keys (flattened)
    connections_data: Vec<u64>,

    /// Temporary buffers (stored to avoid per-sample allocation)
    temp_input_values: Vec<f32>,
    temp_events_buffer: Vec<super::super::traits::PendingEvent>,

    /// Sample rate
    sample_rate: f32,
}

impl GraphStateBuilder {
    /// Create a new builder for the given graph IR
    pub fn new(ir: &GraphIR, nodes: &mut SlotMap<super::super::types::NodeKey, NodeData>) -> Self {
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

            // IMPORTANT: Get inputs in the ORIGINAL order from node_data.inputs
            // NOT from the type-separated arrays in the IR!
            // The node expects inputs in definition order, which may interleave types.
            let node_key = super::super::types::NodeKey::from(slotmap::KeyData::from_ffi(node_ir.key_data));
            let node_inputs = if let Some(node_data) = nodes.get(node_key) {
                node_data.inputs.iter().map(|k| k.data().as_ffi()).collect::<Vec<_>>()
            } else {
                // Fallback to IR order if node not found (shouldn't happen)
                let mut fallback = Vec::new();
                fallback.extend(&node_ir.stream_inputs);
                fallback.extend(&node_ir.value_inputs);
                fallback.extend(&node_ir.event_inputs);
                fallback
            };

            // Collect this node's output keys (stream, value, event)
            let mut node_outputs = Vec::new();
            for &output_key_data in &node_ir.stream_outputs {
                node_outputs.push(output_key_data);
            }
            for &output_key_data in &node_ir.value_outputs {
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

        // Build connections mapping: output endpoint -> [connected input endpoints]
        // We need to map from the endpoint_keys index to connections
        use std::collections::HashMap;
        let mut endpoint_connections: HashMap<u64, Vec<u64>> = HashMap::new();

        for conn in &ir.connections {
            // Get the source output endpoint key
            // IMPORTANT: conn.src_output and conn.dst_input are OVERALL indices,
            // but the IR stores them in type-specific arrays. We need to look up
            // the endpoint key directly from the combined output/input lists.
            let src_node = &ir.nodes[conn.src_node];
            let dst_node = &ir.nodes[conn.dst_node];

            // For source: Get actual node outputs in definition order
            let src_node_key = super::super::types::NodeKey::from(slotmap::KeyData::from_ffi(src_node.key_data));
            let src_combined_outputs = if let Some(node_data) = nodes.get(src_node_key) {
                node_data.outputs.iter().map(|k| k.data().as_ffi()).collect::<Vec<_>>()
            } else {
                // Fallback to IR order if node not found
                let mut fallback = Vec::new();
                fallback.extend(&src_node.stream_outputs);
                fallback.extend(&src_node.value_outputs);
                fallback.extend(&src_node.event_outputs);
                fallback
            };

            // For destination: Get actual node inputs in definition order
            let dst_node_key = super::super::types::NodeKey::from(slotmap::KeyData::from_ffi(dst_node.key_data));
            let dst_combined_inputs = if let Some(node_data) = nodes.get(dst_node_key) {
                node_data.inputs.iter().map(|k| k.data().as_ffi()).collect::<Vec<_>>()
            } else {
                // Fallback to IR order if node not found
                let mut fallback = Vec::new();
                fallback.extend(&dst_node.stream_inputs);
                fallback.extend(&dst_node.value_inputs);
                fallback.extend(&dst_node.event_inputs);
                fallback
            };

            let src_output_key = src_combined_outputs.get(conn.src_output).copied();
            let dst_input_key = dst_combined_inputs.get(conn.dst_input).copied();

            if let (Some(src_key), Some(dst_key)) = (src_output_key, dst_input_key) {
                endpoint_connections.entry(src_key).or_default().push(dst_key);
            }
        }

        // Flatten connections into offset + data arrays
        // For each output endpoint in endpoint_keys[outputs_start..], store its connections
        let mut connections_offsets = Vec::new();
        let mut connections_data = Vec::new();

        connections_offsets.push(0);

        for i in outputs_start..endpoint_keys.len() {
            let output_key = endpoint_keys[i];
            if let Some(connected_inputs) = endpoint_connections.get(&output_key) {
                connections_data.extend(connected_inputs);
            }
            connections_offsets.push(connections_data.len());
        }

        // Allocate temporary buffers (sized for worst case)
        let max_inputs = 32; // MAX_NODE_ENDPOINTS
        let temp_input_values = vec![0.0f32; max_inputs];
        let temp_events_buffer = Vec::with_capacity(64);

        Self {
            node_keys,
            endpoint_keys,
            input_offsets,
            output_offsets,
            connections_offsets,
            connections_data,
            temp_input_values,
            temp_events_buffer,
            sample_rate: ir.sample_rate,
        }
    }

    /// Build the GraphState that can be passed to JIT code
    ///
    /// Returns both GraphState and GraphStateTemps. The GraphStateTemps struct
    /// owns temporary buffers that must live as long as GraphState is in use.
    ///
    /// The temporary buffers are small (512 bytes) and allocated fresh on each call
    /// to keep GraphStateBuilder Send-safe without storing raw pointers.
    pub fn build(
        &mut self,
        nodes: &mut SlotMap<super::super::types::NodeKey, NodeData>,
        endpoints: &mut SlotMap<ValueKey, EndpointState>,
    ) -> (GraphState, GraphStateTemps) {
        // Allocate temporary buffers for pointer arrays
        const MAX_INPUTS: usize = 32;

        // temp_value_inputs holds Option<&ValueData> (thin pointers, 8 bytes each)
        let mut temp_value_inputs = vec![std::ptr::null(); MAX_INPUTS];

        let state = GraphState {
            nodes_slotmap: nodes as *mut _,
            node_keys: self.node_keys.as_ptr(),
            endpoints: endpoints as *mut _,
            endpoint_keys: self.endpoint_keys.as_ptr(),
            input_offsets: self.input_offsets.as_ptr(),
            output_offsets: self.output_offsets.as_ptr(),
            connections_offsets: self.connections_offsets.as_ptr(),
            connections_data: self.connections_data.as_ptr(),
            sample_rate: self.sample_rate,
            node_count: self.node_keys.len(),
            temp_input_values: self.temp_input_values.as_mut_ptr(),
            temp_value_inputs: temp_value_inputs.as_mut_ptr(),
            temp_event_inputs: std::ptr::null_mut(), // Not used - we'll create locally in trampoline
            temp_events_buffer: &mut self.temp_events_buffer as *mut _,
        };

        let temps = GraphStateTemps {
            _temp_value_inputs: temp_value_inputs,
        };

        (state, temps)
    }
}

/// Temporary buffers that must live as long as GraphState
///
/// This struct owns the temporary pointer arrays that GraphState references.
/// Keep this in scope while using GraphState, then drop both together.
pub struct GraphStateTemps {
    _temp_value_inputs: Vec<*const ()>,
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
            for (i, &_input_type) in node_data.input_types.iter().enumerate() {
                if i >= input_keys.len() { break; }

                let value_key = ValueKey::from(slotmap::KeyData::from_ffi(input_keys[i]));
                if let Some(endpoint_state) = endpoints.get(value_key) {
                    // Fill stream_input_values for all types
                    stream_input_values[i] = endpoint_state.as_scalar().unwrap_or(0.0);
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

            // Build event_inputs array using OVERALL indices
            // Use a fixed-size array on the stack to avoid fat pointer issues
            const MAX_INPUTS: usize = 32;
            let mut event_input_values: [&[EventInstance]; MAX_INPUTS] = [&[]; MAX_INPUTS];

            // Fill event slices for Event-type inputs
            for (i, &input_type) in node_data.input_types.iter().enumerate() {
                if i >= input_keys.len() || i >= MAX_INPUTS { break; }

                if input_type == super::super::types::EndpointType::Event {
                    let value_key = ValueKey::from(slotmap::KeyData::from_ffi(input_keys[i]));
                    if let Some(endpoint_state) = endpoints.get(value_key) {
                        if let super::super::types::EndpointState::Event(event_data) = endpoint_state {
                            let events = event_data.queue().events();
                            event_input_values[i] = events;

                            // Debug: Log event delivery for first few frames
                            static mut EVENT_LOG_COUNT: usize = 0;
                            unsafe {
                                if EVENT_LOG_COUNT < 50 && !events.is_empty() {
                                    eprintln!("[JIT EVENT] Node {} input {} received {} events",
                                        node_index, i, events.len());
                                    EVENT_LOG_COUNT += 1;
                                }
                            }
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

            let event_slice = if num_inputs > 0 {
                &event_input_values[..num_inputs]
            } else {
                &[]
            };

            let mut context = ProcessingContext::new(
                stream_slice,
                value_slice,
                event_slice,
                events_buffer,
            );

            // Call the actual process method
            let output = node_data.processor.process(sample_rate, &mut context);

            // Clear event input queues after consumption
            for (i, &input_type) in node_data.input_types.iter().enumerate() {
                if i >= input_keys.len() { break; }

                if input_type == super::super::types::EndpointType::Event {
                    let value_key = ValueKey::from(slotmap::KeyData::from_ffi(input_keys[i]));
                    if let Some(endpoint_state) = endpoints.get_mut(value_key) {
                        if let Some(event_data) = endpoint_state.as_event_mut() {
                            event_data.queue_mut().clear();
                        }
                    }
                }
            }

            // Handle event outputs if any were emitted
            if !events_buffer.is_empty() {
                // Debug: Log event emission
                static mut EVENT_EMIT_LOG_COUNT: usize = 0;
                unsafe {
                    if EVENT_EMIT_LOG_COUNT < 20 {
                        eprintln!("[JIT EVENT EMIT] Node {} emitted {} events", node_index, events_buffer.len());
                        EVENT_EMIT_LOG_COUNT += 1;
                    }
                }

                // Calculate the base output index for this node in the global output array
                let mut total_outputs_before = 0;
                for i in 0..node_index {
                    let node_output_start = *state.output_offsets.add(i);
                    let node_output_end = *state.output_offsets.add(i + 1);
                    total_outputs_before += node_output_end - node_output_start;
                }

                for pending in events_buffer.iter() {
                    // IMPORTANT: pending.output_index is the OVERALL output index (not event-specific)!
                    // Nodes call emit_event with their overall output index (e.g., for outputs=[Value, Event],
                    // emitting on the Event output uses index 1, not event-specific index 0).
                    let output_idx = pending.output_index;

                    // Debug: Log the mapping
                    static mut EVENT_MAP_LOG_COUNT: usize = 0;
                    unsafe {
                        if EVENT_MAP_LOG_COUNT < 20 {
                            eprintln!("[JIT EVENT MAP] Node {} output_idx={}, output_types={:?}",
                                node_index, output_idx, node_data.output_types.as_slice());
                            EVENT_MAP_LOG_COUNT += 1;
                        }
                    }

                    // Verify the output index is in range and is an event type
                    if output_idx >= node_data.output_types.len() {
                        eprintln!("[JIT EVENT ERROR] Node {} output_idx {} out of range (max {})",
                            node_index, output_idx, node_data.output_types.len());
                        continue;
                    }

                    // Verify it's actually an event output
                    if node_data.output_types.get(output_idx) != Some(&super::super::types::EndpointType::Event) {
                        continue;
                    }

                    // Get the event output endpoint
                    if let Some(&event_output_key) = node_data.outputs.get(output_idx) {
                        // Push event to output endpoint
                        if let Some(state_ref) = endpoints.get_mut(event_output_key) {
                            if let Some(event_state) = state_ref.as_event_mut() {
                                let _ = event_state.queue_mut().push(pending.event.clone());
                            }
                        }

                        // Propagate event to connected inputs
                        let global_output_idx = total_outputs_before + output_idx;

                        let conn_start = *state.connections_offsets.add(global_output_idx);
                        let conn_end = *state.connections_offsets.add(global_output_idx + 1);

                        // Debug: Log event propagation
                        static mut EVENT_PROP_LOG_COUNT: usize = 0;
                        unsafe {
                            if EVENT_PROP_LOG_COUNT < 20 && conn_end > conn_start {
                                eprintln!("[JIT EVENT PROP] Node {} output {} propagating to {} connections",
                                    node_index, output_idx, conn_end - conn_start);
                                EVENT_PROP_LOG_COUNT += 1;
                            }
                        }

                        for i in conn_start..conn_end {
                            let connected_input_key_data = *state.connections_data.add(i);
                            let connected_input_key = ValueKey::from(
                                slotmap::KeyData::from_ffi(connected_input_key_data)
                            );

                            if let Some(input_state) = endpoints.get_mut(connected_input_key) {
                                if let Some(input_event_state) = input_state.as_event_mut() {
                                    let _ = input_event_state
                                        .queue_mut()
                                        .push(pending.event.clone());
                                }
                            }
                        }
                    }
                }
                events_buffer.clear();
            }

            output
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

            // Debug: Log non-zero outputs for first few times
            static mut OUTPUT_LOG_COUNT: usize = 0;
            unsafe {
                if OUTPUT_LOG_COUNT < 50 && output_value.abs() > 0.001 {
                    eprintln!("[JIT OUTPUT] Node {} produced output: {:.6}", node_index, output_value);
                    OUTPUT_LOG_COUNT += 1;
                }
            }
        } else {
            return;
        }

        // Find the index of this output in the connections array
        // Count how many outputs come before this one
        let mut total_outputs_before = 0;
        for i in 0..node_index {
            let node_output_start = *state.output_offsets.add(i);
            let node_output_end = *state.output_offsets.add(i + 1);
            total_outputs_before += node_output_end - node_output_start;
        }

        // Look up connections for this output
        let conn_start = *state.connections_offsets.add(total_outputs_before);
        let conn_end = *state.connections_offsets.add(total_outputs_before + 1);

        // Copy to all connected inputs
        for i in conn_start..conn_end {
            let connected_input_key_data = *state.connections_data.add(i);
            let connected_input_key = ValueKey::from(slotmap::KeyData::from_ffi(connected_input_key_data));

            if let Some(input_endpoint) = endpoints.get_mut(connected_input_key) {
                input_endpoint.set_scalar(output_value);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_graph_state_layout() {
        // Verify GraphState is repr(C) and has stable layout
        // Count fields:
        // - nodes_slotmap, node_keys, endpoints, endpoint_keys (4 pointers)
        // - input_offsets, output_offsets (2 pointers)
        // - connections_offsets, connections_data (2 pointers - NEW)
        // - temp_input_values, temp_value_inputs, temp_event_inputs, temp_events_buffer (4 pointers)
        // = 12 pointers * 8 = 96 bytes
        // + sample_rate (f32 = 4 bytes) + node_count (usize = 8 bytes) = 96 + 4 + 8 = 108 bytes
        // With padding to 8-byte boundary: 112 bytes
        assert_eq!(mem::size_of::<GraphState>(), 112);
    }
}
