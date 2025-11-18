use super::traits::ProcessingContext;
use super::types::{EndpointDescriptor, EndpointDirection, EndpointType, EventPayload, ValueInput};
use super::*;
use crate::delay::Delay;
use crate::filters::tpt::TptFilter;
use crate::oscillators::Oscillator;
use crate::Node;
use arrayvec::ArrayVec;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

#[test]
fn test_simple_chain_topology() {
    let mut graph = Graph::new(44100.0);

    let osc = graph.add_node(Oscillator::sine(440.0, 1.0));
    let filter = graph.add_node(TptFilter::new(1000.0, 0.7));

    graph.connect(osc.output, filter.input);

    assert!(graph.validate().is_ok());
    assert!(graph.process().is_ok());
}

#[test]
fn test_invalid_cycle_without_delay() {
    let mut graph = Graph::new(44100.0);

    let osc = graph.add_node(Oscillator::sine(440.0, 1.0));
    let filter = graph.add_node(TptFilter::new(1000.0, 0.7));

    graph.connect(osc.output, filter.input);
    graph.connect(filter.output, osc.frequency);

    assert!(graph.validate().is_err());
    if let Err(GraphError::CycleDetected(nodes)) = graph.validate() {
        assert!(!nodes.is_empty());
    }
}

#[test]
fn test_valid_cycle_with_delay() {
    let mut graph = Graph::new(44100.0);

    let osc = graph.add_node(Oscillator::sine(440.0, 1.0));
    let filter = graph.add_node(TptFilter::new(1000.0, 0.7));
    let delay = graph.add_node(Delay::from_seconds(0.5, 0.3, 44100.0));

    graph.connect(osc.output, filter.input);
    graph.connect(filter.output, delay.input);
    graph.connect(delay.output, osc.frequency);

    assert!(graph.validate().is_ok());
    assert!(graph.process().is_ok());
}

#[test]
fn test_nodes_added_out_of_order() {
    let mut graph = Graph::new(44100.0);

    let filter = graph.add_node(TptFilter::new(1000.0, 0.7));
    let osc = graph.add_node(Oscillator::sine(440.0, 1.0));

    graph.connect(osc.output, filter.input);

    assert!(graph.validate().is_ok());
    assert!(graph.process().is_ok());
}

#[test]
fn test_complex_graph_with_multiple_paths() {
    let mut graph = Graph::new(44100.0);

    let osc1 = graph.add_node(Oscillator::sine(440.0, 1.0));
    let osc2 = graph.add_node(Oscillator::sine(880.0, 1.0));
    let filter1 = graph.add_node(TptFilter::new(1000.0, 0.7));
    let filter2 = graph.add_node(TptFilter::new(2000.0, 0.5));

    graph.connect(osc1.output, filter1.input);
    graph.connect(osc2.output, filter2.input);

    assert!(graph.validate().is_ok());
    assert!(graph.process().is_ok());
}

#[test]
fn test_disconnect_removes_connection() {
    let mut graph = Graph::new(44100.0);

    let osc = graph.add_node(Oscillator::sine(440.0, 1.0));
    let filter = graph.add_node(TptFilter::new(1000.0, 0.7));

    assert!(!graph.disconnect(osc.output, filter.input));

    graph.connect(osc.output, filter.input);

    assert!(graph.disconnect(osc.output, filter.input));
    assert!(!graph.disconnect(osc.output, filter.input));

    assert!(graph.process().is_ok());
}

#[test]
fn test_remove_node_clears_connections() {
    let mut graph = Graph::new(44100.0);

    let osc = graph.add_node(Oscillator::sine(440.0, 1.0));
    let filter = graph.add_node(TptFilter::new(1000.0, 0.7));
    let volume = graph.value_param(0.5);

    graph.connect(osc.output, filter.input);
    graph.connect(volume, filter.cutoff);

    let filter_node = filter.node_key();
    assert!(graph.remove_node(filter_node));

    assert!(!graph.disconnect(osc.output, filter.input));

    let replacement = graph.add_node(TptFilter::new(1500.0, 0.8));
    graph.connect(osc.output, replacement.input);
    graph.connect(volume, replacement.cutoff);

    assert!(graph.process().is_ok());
}

#[test]
fn test_audio_endpoints_are_streams() {
    let mut graph = Graph::new(44100.0);

    let osc = graph.add_node(Oscillator::sine(440.0, 1.0));
    let filter = graph.add_node(TptFilter::new(1000.0, 0.7));

    let osc_output = osc.output.key();
    let filter_input = filter.input.key();
    let filter_cutoff = filter.cutoff.key();

    assert_eq!(
        graph.endpoint_types.get(osc_output).copied(),
        Some(EndpointType::Stream)
    );
    assert_eq!(
        graph.endpoint_types.get(filter_input).copied(),
        Some(EndpointType::Stream)
    );
    assert_eq!(
        graph.endpoint_types.get(filter_cutoff).copied(),
        Some(EndpointType::Value)
    );

    assert!(graph.insert_value_input(filter.cutoff, 2000.0).is_some());
    let bogus_value_handle = ValueInput::new(filter.input.endpoint());
    assert!(graph.insert_value_input(bogus_value_handle, 0.0).is_none());
}

#[derive(Debug)]
struct ContextProbeNode {
    #[allow(dead_code)]
    output: f32,
}

#[derive(Copy, Clone)]
struct ProbeEndpoints {
    input: ValueInput,
    output: StreamOutput,
}

// ProbeEndpoints methods removed - fields are accessed directly

impl ContextProbeNode {
    fn new() -> Self {
        Self { output: 0.0 }
    }
}

impl SignalProcessor for ContextProbeNode {
    fn process(&mut self) {
        // Output is already set by NodeIO::read_inputs
        // Nothing to do in process
    }
}

// Manual NodeIO implementation for test node
impl NodeIO for ContextProbeNode {
    fn read_inputs<'a>(&mut self, context: &mut ProcessingContext<'a>) {
        let value = context.value_scalar(0);
        self.output = value;
    }

    fn get_stream_output(&self, index: usize) -> Option<f32> {
        match index {
            0 => Some(self.output),
            _ => None,
        }
    }
}

impl ProcessingNode for ContextProbeNode {
    type Endpoints = ProbeEndpoints;

    const ENDPOINT_DESCRIPTORS: &'static [EndpointDescriptor] = &[
        EndpointDescriptor::new("input", EndpointType::Value, EndpointDirection::Input),
        EndpointDescriptor::new("output", EndpointType::Stream, EndpointDirection::Output),
    ];

    fn create_endpoints(
        _node_key: NodeKey,
        inputs: ArrayVec<ValueKey, MAX_NODE_ENDPOINTS>,
        outputs: ArrayVec<ValueKey, MAX_NODE_ENDPOINTS>,
    ) -> Self::Endpoints {
        let input_key = inputs[0];
        let output_key = outputs[0];
        ProbeEndpoints {
            input: ValueInput::new(InputEndpoint::new(input_key)),
            output: StreamOutput::new(output_key),
        }
    }
}

#[test]
fn test_processing_context_invocation() {
    let mut graph = Graph::new(44100.0);

    let endpoints = graph.add_node(ContextProbeNode::new());
    graph
        .insert_value_input(endpoints.input, 0.75)
        .expect("value endpoint");

    graph.process().expect("graph processes successfully");

    let output = graph
        .get_value(&endpoints.output)
        .expect("output value available");
    eprintln!("Expected: 0.75, Got: {}", output);
    assert!((output - 0.75).abs() < f32::EPSILON);
}

#[derive(Debug)]
struct EventEmitterNode;

#[derive(Copy, Clone)]
struct EventEmitterEndpoints {
    output: StreamOutput,
}

// EventEmitterEndpoints methods removed - field is accessed directly

impl SignalProcessor for EventEmitterNode {
    fn process(&mut self) {
        // Event emission happens in NodeIO::read_inputs
    }
}

// Manual NodeIO implementation for test node
impl NodeIO for EventEmitterNode {
    fn read_inputs<'a>(&mut self, context: &mut ProcessingContext<'a>) {
        context.emit_scalar_event(0, 0, 1.25);
    }
}

impl ProcessingNode for EventEmitterNode {
    type Endpoints = EventEmitterEndpoints;

    const ENDPOINT_DESCRIPTORS: &'static [EndpointDescriptor] = &[EndpointDescriptor::new(
        "output",
        EndpointType::Event,
        EndpointDirection::Output,
    )];

    fn create_endpoints(
        _node_key: NodeKey,
        _inputs: ArrayVec<ValueKey, MAX_NODE_ENDPOINTS>,
        outputs: ArrayVec<ValueKey, MAX_NODE_ENDPOINTS>,
    ) -> Self::Endpoints {
        let output_key = outputs[0];
        EventEmitterEndpoints {
            output: StreamOutput::new(output_key),
        }
    }
}

#[derive(Debug)]
struct EventSinkNode {
    counter: Arc<AtomicUsize>,
}

#[derive(Copy, Clone)]
struct EventSinkEndpoints {
    input: InputEndpoint,
}

// EventSinkEndpoints methods removed - field is accessed directly

impl EventSinkNode {
    fn new(counter: Arc<AtomicUsize>) -> Self {
        Self { counter }
    }
}

impl SignalProcessor for EventSinkNode {
    fn process(&mut self) {
        // Event counting happens in NodeIO::read_inputs
    }
}

// Manual NodeIO implementation for test node
impl NodeIO for EventSinkNode {
    fn read_inputs<'a>(&mut self, context: &mut ProcessingContext<'a>) {
        let events = context.events(0);
        self.counter.store(events.len(), Ordering::SeqCst);
    }
}

impl ProcessingNode for EventSinkNode {
    type Endpoints = EventSinkEndpoints;

    const ENDPOINT_DESCRIPTORS: &'static [EndpointDescriptor] = &[EndpointDescriptor::new(
        "input",
        EndpointType::Event,
        EndpointDirection::Input,
    )];

    fn create_endpoints(
        _node_key: NodeKey,
        inputs: ArrayVec<ValueKey, MAX_NODE_ENDPOINTS>,
        _outputs: ArrayVec<ValueKey, MAX_NODE_ENDPOINTS>,
    ) -> Self::Endpoints {
        let input_key = inputs[0];
        EventSinkEndpoints {
            input: InputEndpoint::new(input_key),
        }
    }
}

#[test]
fn test_event_emission_and_drain() {
    let mut graph = Graph::new(44100.0);

    let emitter_endpoints = graph.add_node(EventEmitterNode);
    let sink_counter = Arc::new(AtomicUsize::new(0));
    let sink_endpoints = graph.add_node(EventSinkNode::new(sink_counter.clone()));

    graph.connect(emitter_endpoints.output, sink_endpoints.input);

    graph.process().expect("graph processes successfully");

    let mut drained = Vec::new();
    graph.drain_events(emitter_endpoints.output, |event| {
        drained.push(event.payload.as_scalar().unwrap_or(0.0));
    });

    assert_eq!(drained.len(), 1);
    assert!((drained[0] - 1.25).abs() < f32::EPSILON);
    assert_eq!(sink_counter.load(Ordering::SeqCst), 1);

    drained.clear();
    graph.drain_events(emitter_endpoints.output, |event| {
        drained.push(event.payload.as_scalar().unwrap_or(0.0));
    });
    assert!(drained.is_empty());
}

#[test]
fn test_queue_event_host_to_node() {
    let mut graph = Graph::new(44100.0);

    let sink_counter = Arc::new(AtomicUsize::new(0));
    let sink_endpoints = graph.add_node(EventSinkNode::new(sink_counter.clone()));

    let queued = graph.queue_event(sink_endpoints.input, 0, EventPayload::scalar(3.5));
    assert!(queued);

    graph.process().expect("graph processes successfully");
    assert_eq!(sink_counter.load(Ordering::SeqCst), 1);

    graph.process().expect("graph processes successfully");
    assert_eq!(sink_counter.load(Ordering::SeqCst), 0);
}

#[derive(Debug, Node)]
struct StreamValueNode {
    #[input(value)]
    value: f32,
    #[output(stream)]
    output: f32,
}

impl StreamValueNode {
    fn new(initial: f32) -> Self {
        Self {
            value: initial,
            output: initial,
        }
    }
}

impl SignalProcessor for StreamValueNode {
    fn process(&mut self) {
        self.output = self.value;
    }
}

#[test]
fn test_function_node_transform_updates_output() {
    let mut graph = Graph::new(44100.0);
    let source = graph.add_node(StreamValueNode::new(0.25));
    let doubled = graph.transform(source.output, |x| x * 2.0);

    graph.process().expect("graph processes successfully");
    let first = graph
        .get_value(&doubled)
        .expect("transform output available");
    assert!((first - 0.5).abs() < 1e-6);

    graph.set_value(source.value, 0.75);
    graph.process().expect("graph processes successfully");
    let second = graph
        .get_value(&doubled)
        .expect("transform output available");
    assert!((second - 1.5).abs() < 1e-6);
}

#[test]
fn test_binary_function_node_combines_inputs() {
    let mut graph = Graph::new(44100.0);
    let left = graph.add_node(StreamValueNode::new(0.3));
    let right = graph.add_node(StreamValueNode::new(0.2));

    let summed = graph.combine(left.output, right.output, |lhs, rhs| lhs + rhs);

    graph.process().expect("graph processes successfully");
    let first = graph.get_value(&summed).expect("combined output available");
    assert!((first - 0.5).abs() < 1e-6);

    graph.set_value(left.value, 0.6);
    graph.set_value(right.value, 0.1);
    graph.process().expect("graph processes successfully");
    let second = graph.get_value(&summed).expect("combined output available");
    assert!((second - 0.7).abs() < 1e-6);
}

#[test]
fn test_poly_blep_oscillator_emits_audio() {
    let mut direct = crate::PolyBlepOscillator::saw(440.0, 0.5);
    let mut standalone_non_zero = 0;
    for _ in 0..64 {
        direct.process(44100.0);
        if direct.output.abs() > 1e-6 {
            standalone_non_zero += 1;
        }
    }
    assert!(
        standalone_non_zero > 0,
        "standalone oscillator did not emit output"
    );

    let mut graph = Graph::new(44100.0);
    let osc = graph.add_node(crate::PolyBlepOscillator::saw(440.0, 0.5));

    let node_data = graph.nodes.get(osc.node_key()).expect("node data");
    assert_eq!(
        node_data.output_types[0],
        EndpointType::Stream,
        "oscillator output not registered as stream"
    );

    let mut non_zero_samples = 0;
    for _ in 0..32 {
        graph.process().expect("graph processes successfully");
        let node_data = graph.nodes.get(osc.node_key()).unwrap();
        let sample_direct = node_data.processor.get_stream_output(0).unwrap_or(0.0);
        let sample = graph.get_value(&osc.output).unwrap_or(0.0);
        if sample.abs() > 1e-6 {
            non_zero_samples += 1;
        }
        assert!(
            (sample - sample_direct).abs() < 1e-6,
            "endpoint sample differs from node output"
        );
    }

    assert!(non_zero_samples > 0, "oscillator output remained silent");
}
