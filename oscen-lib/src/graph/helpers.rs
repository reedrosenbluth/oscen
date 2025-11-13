use arrayvec::ArrayVec;

use super::traits::{ProcessingContext, ProcessingNode, SignalProcessor};
use super::types::{
    EndpointDescriptor, EndpointDirection, EndpointType, NodeKey, ValueKey, MAX_NODE_ENDPOINTS,
};

#[derive(Debug)]
pub struct FunctionNode {}

impl FunctionNode {
    pub fn new(_f: fn(f32) -> f32) -> Self {
        Self {}
    }
}

impl SignalProcessor for FunctionNode {
    fn process(&mut self, _sample_rate: f32) {
        // FunctionNode doesn't support the new API properly yet
        // For now, process is a no-op since we don't have access to inputs/outputs
        // This node needs to be refactored to use the struct-of-arrays pattern
    }
}

// Manual NodeIO implementation for FunctionNode
impl super::traits::NodeIO for FunctionNode {
    fn read_inputs<'a>(&mut self, _context: &mut ProcessingContext<'a>) {
        // FunctionNode doesn't support the new API properly yet
        // This node needs to be refactored to use the struct-of-arrays pattern
    }
}

impl ProcessingNode for FunctionNode {
    type Endpoints = NodeKey;

    const ENDPOINT_DESCRIPTORS: &'static [EndpointDescriptor] = &[
        EndpointDescriptor::new("input", EndpointType::Stream, EndpointDirection::Input),
        EndpointDescriptor::new("output", EndpointType::Stream, EndpointDirection::Output),
    ];

    fn create_endpoints(
        node_key: NodeKey,
        _inputs: ArrayVec<ValueKey, MAX_NODE_ENDPOINTS>,
        _outputs: ArrayVec<ValueKey, MAX_NODE_ENDPOINTS>,
    ) -> Self::Endpoints {
        node_key
    }
}

#[derive(Debug)]
pub struct BinaryFunctionNode {}

impl BinaryFunctionNode {
    pub fn new(_f: fn(f32, f32) -> f32) -> Self {
        Self {}
    }
}

impl SignalProcessor for BinaryFunctionNode {
    fn process(&mut self, _sample_rate: f32) {
        // BinaryFunctionNode doesn't support the new API properly yet
        // For now, process is a no-op since we don't have access to inputs/outputs
        // This node needs to be refactored to use the struct-of-arrays pattern
    }
}

// Manual NodeIO implementation for BinaryFunctionNode
impl super::traits::NodeIO for BinaryFunctionNode {
    fn read_inputs<'a>(&mut self, _context: &mut ProcessingContext<'a>) {
        // BinaryFunctionNode doesn't support the new API properly yet
        // This node needs to be refactored to use the struct-of-arrays pattern
    }
}

impl ProcessingNode for BinaryFunctionNode {
    type Endpoints = NodeKey;

    const ENDPOINT_DESCRIPTORS: &'static [EndpointDescriptor] = &[
        EndpointDescriptor::new("lhs", EndpointType::Stream, EndpointDirection::Input),
        EndpointDescriptor::new("rhs", EndpointType::Stream, EndpointDirection::Input),
        EndpointDescriptor::new("output", EndpointType::Stream, EndpointDirection::Output),
    ];

    fn create_endpoints(
        node_key: NodeKey,
        _inputs: ArrayVec<ValueKey, MAX_NODE_ENDPOINTS>,
        _outputs: ArrayVec<ValueKey, MAX_NODE_ENDPOINTS>,
    ) -> Self::Endpoints {
        node_key
    }
}
