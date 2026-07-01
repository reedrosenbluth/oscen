use oscen::{graph, PolyBlepOscillator};

// A call argument naming a node that doesn't exist (`ocs` is a typo for
// `osc`) must be a compile error, not a silently dropped connection that
// renders silence at runtime.
fn double(x: f32) -> f32 {
    x * 2.0
}

graph! {
    name: UnknownNodeInCallArg;
    output stream out;
    nodes {
        osc = PolyBlepOscillator::saw(440.0, 0.6);
    }
    connections {
        double(ocs.output) -> out;
    }
}

fn main() {}
