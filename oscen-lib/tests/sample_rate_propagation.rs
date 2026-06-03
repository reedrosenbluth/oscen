#![feature(inherent_associated_types)]

use oscen::{graph, Node, SampleRate, SignalProcessor, StreamOutput};

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
