use oscen::{graph, Gain, PolyBlepOscillator};

// An expression mixing a base-rate node and an oversampled node has no single
// clock. Previously the leftmost operand silently decided the resampling
// policy, so swapping the operands changed the audio. Both spellings must be
// rejected with the same diagnostic.
graph! {
    name: MixedClock;
    output stream out;
    nodes {
        slow = PolyBlepOscillator::saw(220.0, 0.5);
        fast = PolyBlepOscillator::saw(440.0, 0.5) * 4;
        sink = Gain::new(1.0);
    }
    connections {
        slow.output + fast.output -> sink.input;
        sink.output -> out;
    }
}

fn main() {}
