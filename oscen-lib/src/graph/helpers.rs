use arrayvec::ArrayVec;

use super::traits::{ProcessingNode, SignalProcessor};
use super::types::{EndpointType, NodeKey, ValueKey, MAX_NODE_ENDPOINTS};

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
    fn process(&mut self, _sample_rate: f32, inputs: &[f32]) -> f32 {
        (self.f)(inputs[0])
    }
}

impl ProcessingNode for FunctionNode {
    type Endpoints = NodeKey;

    const INPUT_TYPES: &'static [EndpointType] = &[EndpointType::Stream];
    const OUTPUT_TYPES: &'static [EndpointType] = &[EndpointType::Stream];

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
    fn process(&mut self, _sample_rate: f32, inputs: &[f32]) -> f32 {
        (self.f)(inputs[0], inputs[1])
    }
}

impl ProcessingNode for BinaryFunctionNode {
    type Endpoints = NodeKey;

    const INPUT_TYPES: &'static [EndpointType] = &[EndpointType::Stream, EndpointType::Stream];
    const OUTPUT_TYPES: &'static [EndpointType] = &[EndpointType::Stream];

    fn create_endpoints(
        node_key: NodeKey,
        _inputs: ArrayVec<ValueKey, MAX_NODE_ENDPOINTS>,
        _outputs: ArrayVec<ValueKey, MAX_NODE_ENDPOINTS>,
    ) -> Self::Endpoints {
        node_key
    }
}
