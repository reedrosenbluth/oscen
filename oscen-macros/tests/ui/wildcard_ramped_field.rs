#![feature(inherent_associated_types)]

use oscen::graph::ValueRampState;
use oscen::{graph, Node, SignalProcessor};

// A voice smoothing its own level with a runtime-configured ramp length:
// the endpoint manifest can only mark it `ramped`, so a wildcard hoist
// cannot re-declare the smoothing and must reject the endpoint.
#[derive(Debug, Node)]
pub struct SmoothVoice {
    #[input(value)]
    pub level: ValueRampState,
    #[output(stream)]
    pub audio: f32,
}

impl SmoothVoice {
    pub fn new() -> Self {
        Self {
            level: ValueRampState::new(0.0),
            audio: 0.0,
        }
    }
}

impl SignalProcessor for SmoothVoice {
    fn process(&mut self) {}
}

graph! {
    name: RampedWild;
    output stream out;
    nodes {
        voice = SmoothVoice::new();
    }
    input voice.*;  // error: `level` declares runtime-length smoothing
    connections {
        voice.audio -> out;
    }
}

fn main() {}
