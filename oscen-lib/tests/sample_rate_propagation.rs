#![feature(inherent_associated_types)]

use oscen::{graph, Node, SampleRate, SignalProcessor, StreamInput, StreamOutput};

// A node with no init() — it relies entirely on the graph calling set_sample_rate.
#[derive(Debug, Node)]
struct RateProbe {
    sample_rate: SampleRate,
    pub out: StreamOutput,
}

impl RateProbe {
    fn new() -> Self {
        Self {
            sample_rate: SampleRate::default(),
            out: StreamOutput::default(),
        }
    }
}

impl SignalProcessor for RateProbe {
    fn process(&mut self) {
        *self.out = *self.sample_rate;
    }
}

graph! {
    name: ProbeGraph;
    output stream out;
    nodes {
        probe = RateProbe::new();
    }
    connections {
        probe.out -> out;
    }
}

#[test]
fn child_receives_graph_sample_rate() {
    let mut g = ProbeGraph::new();
    g.init(48_000.0);
    g.process();
    assert_eq!(g.out, 48_000.0);
}

// The graph's set_sample_rate is rate-only (no resampler reset, no derived
// state), but it must reach every child on its own — without init().
#[test]
fn set_sample_rate_alone_propagates_to_children() {
    let mut g = ProbeGraph::new();
    g.set_sample_rate(48_000.0);
    assert_eq!(*g.probe.sample_rate, 48_000.0);
}

graph! {
    name: OversampledProbeGraph;
    output stream out;
    nodes {
        probe = RateProbe::new() * 2;
    }
    connections {
        [latch] probe.out -> out;
    }
}

// A child inside a `* 2` oversampled group must see the scaled inner rate,
// not the host rate.
#[test]
fn oversampled_child_receives_scaled_rate() {
    let mut g = OversampledProbeGraph::new();
    g.init(48_000.0);
    assert_eq!(*g.probe.sample_rate, 96_000.0);
}

#[test]
fn oversampled_child_rate_scales_via_set_sample_rate_alone() {
    let mut g = OversampledProbeGraph::new();
    g.set_sample_rate(48_000.0);
    assert_eq!(*g.probe.sample_rate, 96_000.0);
}

/// Stream passthrough so the nested graph's output has somewhere to go in the
/// outer graph.
#[derive(Debug, Node)]
struct RateSink {
    pub inp: StreamInput,
    pub out: StreamOutput,
}

impl RateSink {
    fn new() -> Self {
        Self {
            inp: StreamInput::default(),
            out: StreamOutput::default(),
        }
    }
}

impl SignalProcessor for RateSink {
    fn process(&mut self) {
        *self.out = *self.inp;
    }
}

graph! {
    name: InnerRateGraph;
    output stream out;
    nodes {
        probe = RateProbe::new();
    }
    connections {
        probe.out -> out;
    }
}

graph! {
    name: OuterRateGraph;
    output stream out;
    nodes {
        inner = InnerRateGraph::new();
        sink = RateSink::new();
    }
    connections {
        inner.out -> sink.inp;
        sink.out -> out;
    }
}

// Rate distribution must recurse through nested graphs down to grandchildren,
// via init() and via set_sample_rate() alone.
#[test]
fn nested_graph_propagates_rate_to_grandchildren() {
    let mut g = OuterRateGraph::new();
    g.init(48_000.0);
    assert_eq!(*g.inner.probe.sample_rate, 48_000.0);

    let mut g = OuterRateGraph::new();
    g.set_sample_rate(48_000.0);
    assert_eq!(*g.inner.probe.sample_rate, 48_000.0);
}
