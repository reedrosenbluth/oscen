use crate::fm_operator::FmOperator;
use oscen::prelude::*;

// Minimal test graph
graph! {
    name: TestVoice;

    input gate: event;
    output audio_out: stream;

    nodes {
        osc = FmOperator::new();
        env = AdsrEnvelope::new(0.01, 0.1, 0.7, 0.3);
    }

    connections {
        gate -> env.gate;
        osc.output * env.output -> audio_out;
    }
}
