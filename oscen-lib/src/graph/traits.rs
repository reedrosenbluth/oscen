use arrayvec::ArrayVec;

use super::types::NodeKey;
use super::types::{EndpointType, ValueKey, MAX_NODE_ENDPOINTS};

pub trait SignalProcessor: Send + std::fmt::Debug {
    fn init(&mut self, _sample_rate: f32) {}
    fn process(&mut self, sample_rate: f32, inputs: &[f32]) -> f32;

    fn allows_feedback(&self) -> bool {
        false
    }
}

pub trait ProcessingNode: SignalProcessor {
    type Endpoints;

    const INPUT_TYPES: &'static [EndpointType] = &[];
    const OUTPUT_TYPES: &'static [EndpointType] = &[];

    fn create_endpoints(
        node_key: NodeKey,
        inputs: ArrayVec<ValueKey, MAX_NODE_ENDPOINTS>,
        outputs: ArrayVec<ValueKey, MAX_NODE_ENDPOINTS>,
    ) -> Self::Endpoints;
}
