use super::*;
use crate::delay::Delay;
use crate::filters::tpt::TptFilter;
use crate::oscillators::Oscillator;

#[test]
fn test_simple_chain_topology() {
    let mut graph = Graph::new(44100.0);

    let osc = graph.add_node(Oscillator::sine(440.0, 1.0));
    let filter = graph.add_node(TptFilter::new(1000.0, 0.7));

    graph.connect(osc.output(), filter.input());

    assert!(graph.validate().is_ok());
    assert!(graph.process().is_ok());
}

#[test]
fn test_invalid_cycle_without_delay() {
    let mut graph = Graph::new(44100.0);

    let osc = graph.add_node(Oscillator::sine(440.0, 1.0));
    let filter = graph.add_node(TptFilter::new(1000.0, 0.7));

    graph.connect(osc.output(), filter.input());
    graph.connect(filter.output(), osc.frequency());

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
    let delay = graph.add_node(Delay::new(0.5, 0.3));

    graph.connect(osc.output(), filter.input());
    graph.connect(filter.output(), delay.input());
    graph.connect(delay.output(), osc.frequency());

    assert!(graph.validate().is_ok());
    assert!(graph.process().is_ok());
}

#[test]
fn test_nodes_added_out_of_order() {
    let mut graph = Graph::new(44100.0);

    let filter = graph.add_node(TptFilter::new(1000.0, 0.7));
    let osc = graph.add_node(Oscillator::sine(440.0, 1.0));

    graph.connect(osc.output(), filter.input());

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

    graph.connect(osc1.output(), filter1.input());
    graph.connect(osc2.output(), filter2.input());

    assert!(graph.validate().is_ok());
    assert!(graph.process().is_ok());
}

#[test]
fn test_audio_endpoints_are_streams() {
    let mut graph = Graph::new(44100.0);

    let osc = graph.add_node(Oscillator::sine(440.0, 1.0));
    let filter = graph.add_node(TptFilter::new(1000.0, 0.7));

    let osc_output = osc.output().key();
    let filter_input = filter.input().key();
    let filter_cutoff = filter.cutoff().key();

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

    assert!(graph.insert_value_input(filter.cutoff(), 2000.0).is_some());
    assert!(graph.insert_value_input(filter.input(), 0.0).is_none());
}
