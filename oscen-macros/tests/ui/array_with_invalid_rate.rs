use oscen::{graph, PolyBlepOscillator};

graph! {
    name: BadArrayRate;
    output stream out;
    nodes {
        oscs = [PolyBlepOscillator::saw(440.0, 0.6); 4] * 3;  // 3 not in {1,2,4,8}
    }
    connections {
        oscs[0].output -> out;
    }
}

fn main() {}
