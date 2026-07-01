use oscen::{graph, PolyBlepOscillator};

// `Frame::<2>` without an argument list is a malformed constructor; it must
// be a parse error, not silently degrade to an unknown bare ident whose
// connection is dropped.
graph! {
    name: FrameTurbofishNoArgs;
    output stream out;
    nodes {
        osc = PolyBlepOscillator::saw(440.0, 0.6);
    }
    connections {
        Frame::<2> -> out;
    }
}

fn main() {}
