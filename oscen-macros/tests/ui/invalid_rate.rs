use oscen::{graph, PolyBlepOscillator};

graph! {
    name: BadRate;
    output stream out;
    nodes {
        osc = PolyBlepOscillator::saw(440.0, 0.6) * 3;  // 3 is not in {1, 2, 4, 8}
    }
    connections {
        osc.output -> out;
    }
}

fn main() {}
