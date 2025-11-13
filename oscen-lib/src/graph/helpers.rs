use arrayvec::ArrayVec;

use super::graph_impl::DynamicIO;
use super::traits::{IOStructAccess, ProcessingContext, ProcessingNode, SignalProcessor};
use super::types::{
    EndpointDescriptor, EndpointDirection, EndpointType, NodeKey, ValueKey, MAX_NODE_ENDPOINTS,
};

/// Helper node that applies a function to transform a stream input
pub struct FunctionNode {
    // Input and output fields for struct-of-arrays pattern
    input: f32,
    output: f32,
    // Function to apply
    f: fn(f32) -> f32,
}

impl std::fmt::Debug for FunctionNode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FunctionNode")
            .field("input", &self.input)
            .field("output", &self.output)
            .field("f", &"<fn>")
            .finish()
    }
}

impl FunctionNode {
    pub fn new(f: fn(f32) -> f32) -> Self {
        Self {
            input: 0.0,
            output: 0.0,
            f,
        }
    }

    pub const CREATE_IO_FN: fn() -> Box<dyn IOStructAccess> = || {
        Box::new(DynamicIO::new(1, 1)) // 1 stream input, 1 stream output
    };
}

impl SignalProcessor for FunctionNode {
    #[inline(always)]
    fn process(&mut self, _sample_rate: f32) {
        // Apply the function to transform the input
        self.output = (self.f)(self.input);
    }
}

// Manual NodeIO implementation for FunctionNode
impl super::traits::NodeIO for FunctionNode {
    #[inline(always)]
    fn read_inputs<'a>(&mut self, context: &mut ProcessingContext<'a>) {
        // Read stream input at index 0
        self.input = context.stream(0);
    }

    #[inline(always)]
    fn get_stream_output(&self, index: usize) -> Option<f32> {
        if index == 0 {
            Some(self.output)
        } else {
            None
        }
    }

    #[inline(always)]
    fn set_stream_input(&mut self, index: usize, value: f32) {
        if index == 0 {
            self.input = value;
        }
    }
}

impl ProcessingNode for FunctionNode {
    type Endpoints = NodeKey;

    const ENDPOINT_DESCRIPTORS: &'static [EndpointDescriptor] = &[
        EndpointDescriptor::new("input", EndpointType::Stream, EndpointDirection::Input),
        EndpointDescriptor::new("output", EndpointType::Stream, EndpointDirection::Output),
    ];

    const CREATE_IO_FN: fn() -> Box<dyn IOStructAccess> = FunctionNode::CREATE_IO_FN;

    fn create_endpoints(
        node_key: NodeKey,
        _inputs: ArrayVec<ValueKey, MAX_NODE_ENDPOINTS>,
        _outputs: ArrayVec<ValueKey, MAX_NODE_ENDPOINTS>,
    ) -> Self::Endpoints {
        node_key
    }
}

/// Helper node that applies a binary function to combine two stream inputs
pub struct BinaryFunctionNode {
    // Input and output fields for struct-of-arrays pattern
    lhs: f32,
    rhs: f32,
    output: f32,
    // Function to apply
    f: fn(f32, f32) -> f32,
}

impl std::fmt::Debug for BinaryFunctionNode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BinaryFunctionNode")
            .field("lhs", &self.lhs)
            .field("rhs", &self.rhs)
            .field("output", &self.output)
            .field("f", &"<fn>")
            .finish()
    }
}

impl BinaryFunctionNode {
    pub fn new(f: fn(f32, f32) -> f32) -> Self {
        Self {
            lhs: 0.0,
            rhs: 0.0,
            output: 0.0,
            f,
        }
    }

    pub const CREATE_IO_FN: fn() -> Box<dyn IOStructAccess> = || {
        Box::new(DynamicIO::new(2, 1)) // 2 stream inputs, 1 stream output
    };
}

impl SignalProcessor for BinaryFunctionNode {
    #[inline(always)]
    fn process(&mut self, _sample_rate: f32) {
        // Apply the binary function to combine the inputs
        self.output = (self.f)(self.lhs, self.rhs);
    }
}

// Manual NodeIO implementation for BinaryFunctionNode
impl super::traits::NodeIO for BinaryFunctionNode {
    #[inline(always)]
    fn read_inputs<'a>(&mut self, context: &mut ProcessingContext<'a>) {
        // Read stream inputs at indices 0 and 1
        self.lhs = context.stream(0);
        self.rhs = context.stream(1);
    }

    #[inline(always)]
    fn get_stream_output(&self, index: usize) -> Option<f32> {
        if index == 0 {
            Some(self.output)
        } else {
            None
        }
    }

    #[inline(always)]
    fn set_stream_input(&mut self, index: usize, value: f32) {
        match index {
            0 => self.lhs = value,
            1 => self.rhs = value,
            _ => {}
        }
    }
}

impl ProcessingNode for BinaryFunctionNode {
    type Endpoints = NodeKey;

    const ENDPOINT_DESCRIPTORS: &'static [EndpointDescriptor] = &[
        EndpointDescriptor::new("lhs", EndpointType::Stream, EndpointDirection::Input),
        EndpointDescriptor::new("rhs", EndpointType::Stream, EndpointDirection::Input),
        EndpointDescriptor::new("output", EndpointType::Stream, EndpointDirection::Output),
    ];

    const CREATE_IO_FN: fn() -> Box<dyn IOStructAccess> = BinaryFunctionNode::CREATE_IO_FN;

    fn create_endpoints(
        node_key: NodeKey,
        _inputs: ArrayVec<ValueKey, MAX_NODE_ENDPOINTS>,
        _outputs: ArrayVec<ValueKey, MAX_NODE_ENDPOINTS>,
    ) -> Self::Endpoints {
        node_key
    }
}
