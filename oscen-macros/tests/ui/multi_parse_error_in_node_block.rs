use oscen::graph;

graph! {
    name: BadNodeBlock;

    input stream s;
    output stream out;

    nodes {
        osc = PolyBlepOscillator::saw(440.0, 0.6)
        lfo = PolyBlepOscillator::sine(2.0, 0.5);
        amp $ 0.8;
    }

    connections {
        s -> out;
    }
}

fn main() {}
