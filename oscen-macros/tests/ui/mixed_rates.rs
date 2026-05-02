use oscen::{graph, PolyBlepOscillator, TptFilter};

graph! {
    name: BadMix;
    output stream out;
    nodes {
        a = PolyBlepOscillator::saw(440.0, 0.6) * 4;
        b = TptFilter::new(1000.0, 0.7) * 2;
    }
    connections {
        a.output -> b.input;  // 4 -> 2: not supported in v1
        b.output -> out;
    }
}

fn main() {}
