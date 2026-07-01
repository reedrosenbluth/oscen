use oscen::{graph, PolyBlepOscillator};

// Turbofish arguments are dropped at parse time, so allowing them on an
// arbitrary function would silently discard the user's generic argument.
// Only the `Frame` constructor may take one.
graph! {
    name: TurbofishNonFrame;
    output stream out;
    nodes {
        osc = PolyBlepOscillator::saw(440.0, 0.6);
    }
    connections {
        dsp::split::<4>(osc.output) -> out;
    }
}

fn main() {}
