use arrayvec::ArrayVec;

use super::traits::{ProcessingContext, ProcessingNode, SignalProcessor};
use super::types::{
    EndpointDescriptor, EndpointDirection, EndpointType, NodeKey, ValueKey, MAX_NODE_ENDPOINTS,
};

#[derive(Debug)]
pub struct FunctionNode {
    pub(crate) f: fn(f32) -> f32,
}

impl FunctionNode {
    pub fn new(f: fn(f32) -> f32) -> Self {
        Self { f }
    }
}

impl SignalProcessor for FunctionNode {
    fn process<'a>(
        &mut self,
        _sample_rate: f32,
        context: &mut ProcessingContext<'a>,
    ) {
        // FunctionNode doesn't support the new API properly yet
        // For now, just call the function but don't return anything
        let _result = (self.f)(context.stream(0));
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
pub struct BinaryFunctionNode {
    pub(crate) f: fn(f32, f32) -> f32,
}

impl BinaryFunctionNode {
    pub fn new(f: fn(f32, f32) -> f32) -> Self {
        Self { f }
    }
}

impl SignalProcessor for BinaryFunctionNode {
    fn process<'a>(
        &mut self,
        _sample_rate: f32,
        context: &mut ProcessingContext<'a>,
    ) {
        // BinaryFunctionNode doesn't support the new API properly yet
        // For now, just call the function but don't return anything
        let _result = (self.f)(context.stream(0), context.stream(1));
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
