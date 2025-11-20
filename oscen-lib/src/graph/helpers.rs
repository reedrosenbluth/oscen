use arrayvec::ArrayVec;

use super::traits::{DynNode, IOStructAccess, ProcessingContext, ProcessingNode, SignalProcessor};
use super::types::{
    EndpointDescriptor, EndpointDirection, EndpointType, EventInstance, NodeKey, ValueKey,
    MAX_NODE_ENDPOINTS,
};

#[derive(Debug)]
pub struct FunctionNode {
    func: fn(f32) -> f32,
    input: f32,
    output: f32,
}

impl FunctionNode {
    pub fn new(f: fn(f32) -> f32) -> Self {
        Self {
            func: f,
            input: 0.0,
            output: 0.0,
        }
    }
}

impl SignalProcessor for FunctionNode {
    fn process(&mut self) {
        self.output = (self.func)(self.input);
    }
}

// Manual NodeIO implementation for FunctionNode
impl super::traits::NodeIO for FunctionNode {
    fn read_inputs<'a>(&mut self, context: &mut ProcessingContext<'a>) {
        self.input = context.stream(0);
    }

    fn get_stream_output(&self, index: usize) -> Option<f32> {
        match index {
            0 => Some(self.output),
            _ => None,
        }
    }

    fn set_stream_input(&mut self, index: usize, value: f32) {
        if index == 0 {
            self.input = value;
        }
    }
}

#[derive(Debug, Default)]
struct FunctionNodeIO {
    input: f32,
    output: f32,
}

impl IOStructAccess for FunctionNodeIO {
    fn num_stream_inputs(&self) -> usize {
        1
    }

    fn num_stream_outputs(&self) -> usize {
        1
    }

    fn num_event_outputs(&self) -> usize {
        0
    }

    fn set_stream_input(&mut self, index: usize, value: f32) {
        if index == 0 {
            self.input = value;
        }
    }

    fn get_stream_input(&self, index: usize) -> Option<f32> {
        match index {
            0 => Some(self.input),
            _ => None,
        }
    }

    fn set_stream_output(&mut self, index: usize, value: f32) {
        if index == 0 {
            self.output = value;
        }
    }

    fn get_stream_output(&self, index: usize) -> Option<f32> {
        match index {
            0 => Some(self.output),
            _ => None,
        }
    }

    fn get_event_output(&self, _index: usize) -> &[EventInstance] {
        &[]
    }

    fn clear_event_outputs(&mut self) {}
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

    const CREATE_IO_FN: fn() -> Box<dyn IOStructAccess> = || Box::new(FunctionNodeIO::default());
}

#[derive(Debug)]
pub struct BinaryFunctionNode {
    func: fn(f32, f32) -> f32,
    lhs: f32,
    rhs: f32,
    output: f32,
}

impl BinaryFunctionNode {
    pub fn new(f: fn(f32, f32) -> f32) -> Self {
        Self {
            func: f,
            lhs: 0.0,
            rhs: 0.0,
            output: 0.0,
        }
    }
}

impl SignalProcessor for BinaryFunctionNode {
    fn process(&mut self) {
        self.output = (self.func)(self.lhs, self.rhs);
    }
}

// Manual NodeIO implementation for BinaryFunctionNode
impl super::traits::NodeIO for BinaryFunctionNode {
    fn read_inputs<'a>(&mut self, context: &mut ProcessingContext<'a>) {
        self.lhs = context.stream(0);
        self.rhs = context.stream(1);
    }

    fn get_stream_output(&self, index: usize) -> Option<f32> {
        match index {
            0 => Some(self.output),
            _ => None,
        }
    }

    fn set_stream_input(&mut self, index: usize, value: f32) {
        match index {
            0 => {
                self.lhs = value;
            }
            1 => {
                self.rhs = value;
            }
            _ => {}
        }
    }
}

#[derive(Debug, Default)]
struct BinaryFunctionNodeIO {
    lhs: f32,
    rhs: f32,
    output: f32,
}

impl IOStructAccess for BinaryFunctionNodeIO {
    fn num_stream_inputs(&self) -> usize {
        2
    }

    fn num_stream_outputs(&self) -> usize {
        1
    }

    fn num_event_outputs(&self) -> usize {
        0
    }

    fn set_stream_input(&mut self, index: usize, value: f32) {
        match index {
            0 => self.lhs = value,
            1 => self.rhs = value,
            _ => {}
        }
    }

    fn get_stream_input(&self, index: usize) -> Option<f32> {
        match index {
            0 => Some(self.lhs),
            1 => Some(self.rhs),
            _ => None,
        }
    }

    fn set_stream_output(&mut self, index: usize, value: f32) {
        if index == 0 {
            self.output = value;
        }
    }

    fn get_stream_output(&self, index: usize) -> Option<f32> {
        match index {
            0 => Some(self.output),
            _ => None,
        }
    }

    fn get_event_output(&self, _index: usize) -> &[EventInstance] {
        &[]
    }

    fn clear_event_outputs(&mut self) {}
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

    const CREATE_IO_FN: fn() -> Box<dyn IOStructAccess> =
        || Box::new(BinaryFunctionNodeIO::default());
}

// DynNode implementations for helper nodes
impl DynNode for FunctionNode {}
impl DynNode for BinaryFunctionNode {}
