use oscen::{graph, PolyBlepOscillator};

graph! {
    name: MixedFailure;

    input stream s;

    output value v_out;

    nodes {
        osc = PolyBlepOscillator::saw(440.0, 0.6) / 2;
    }

    connections {
        s -> v_out;
    }
}

fn main() {}
