//! Regression test for the "both unknown" heuristic trap. Constructs a
//! cross-rate stream edge where neither endpoint is anchored to a graph-level
//! input or output. Pre-fix: the macro routed this through the same-rate event
//! path silently, dropping the resampler. Post-fix (heuristic deleted): the
//! edge classifies as Up/Down with Default policy, and the concrete-kernel
//! emitter produces a SincUpFir/SincDownFir resampler.

use oscen::graph::{StreamInput, StreamOutput};
use oscen::{graph, Node, SignalProcessor};

#[derive(Debug, Node)]
pub struct UnitGain {
    pub input: StreamInput,
    pub output: StreamOutput,
}

impl UnitGain {
    pub fn new() -> Self {
        Self {
            input: StreamInput::default(),
            output: StreamOutput::default(),
        }
    }
}

impl Default for UnitGain {
    fn default() -> Self {
        Self::new()
    }
}

impl SignalProcessor for UnitGain {
    fn process(&mut self) {
        *self.output = *self.input;
    }
}

#[derive(Debug, Node)]
pub struct ImpulseSource {
    pub output: StreamOutput,
    fired: bool,
}

impl ImpulseSource {
    pub fn new() -> Self {
        Self {
            output: StreamOutput::default(),
            fired: false,
        }
    }
}

impl Default for ImpulseSource {
    fn default() -> Self {
        Self::new()
    }
}

impl SignalProcessor for ImpulseSource {
    fn process(&mut self) {
        if self.fired {
            *self.output = 0.0;
        } else {
            *self.output = 1.0;
            self.fired = true;
        }
    }
}

graph! {
    name: TrapGraph;
    output stream out;
    nodes {
        // ImpulseSource is at outer rate, UnitGain is *4 (inner). The edge
        // imp.output -> ug.input is the trap configuration: both endpoints
        // are node fields with no graph-level anchor and no [policy] keyword.
        // Pre-fix: the heuristic rewrote this to Event { rescale: None },
        // dropping the resampler.
        imp = ImpulseSource::new();
        ug = UnitGain::new() * 4;
    }
    connections {
        imp.output -> ug.input;
        ug.output -> out;
    }
}

#[test]
fn cross_rate_unanchored_stream_edge_resamples() {
    let mut g = TrapGraph::new();
    g.init(48_000.0);
    g.process_block(64);

    // The trap symptom was: a single impulse from `imp` would pass through
    // the unanchored cross-rate edge without filtering, leaving a single
    // nonzero sample in the output. With the trap closed, the SincDownFir<4>
    // smears the impulse over multiple samples (sinc impulse response).
    let observed = &g.out_block[..64];
    let nonzero_count = observed.iter().filter(|&&s| s.abs() > 1e-6).count();
    assert!(
        nonzero_count > 1,
        "expected resampler smear over multiple samples, got {nonzero_count} nonzero samples \
         (the trap's symptom: a single impulse passes through unfiltered)"
    );
}
