use oscen::{graph, PolyBlepOscillator};

graph! {
    name: DoubleRate;
    output stream out;
    nodes {
        oscs = [PolyBlepOscillator::saw(440.0, 0.6); 4] * 2 * 4;  // conflict
    }
    connections {
        oscs[0].output -> out;
    }
}

fn main() {}
