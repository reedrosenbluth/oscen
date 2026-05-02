use oscen::{graph, PolyBlepOscillator};

graph! {
    name: BadDown;
    output stream out;
    nodes {
        osc = PolyBlepOscillator::saw(440.0, 0.6) / 2;
    }
    connections {
        osc.output -> out;
    }
}

fn main() {}
