use oscen::{graph, Frame, PolyBlepOscillator, TptFilter};

// A frame constructor (`Frame::<2>(...)`) is a compound, frame-valued source.
// It is supported into a scalar stream destination, but NOT broadcast into a
// node array: the array-broadcast path binds `let __src: f32`, which a frame
// cannot flow through. Codegen must emit a scoped `compile_error!` rather than a
// confusing `expected f32, found Frame<_>` type error.
graph! {
    name: BadFrameBroadcast;
    output stream out: Frame<2>;
    nodes {
        a = PolyBlepOscillator::saw(440.0, 0.6);
        b = PolyBlepOscillator::saw(220.0, 0.6);
        filters = [TptFilter::<Frame<2>>::new(800.0, 0.7); 2];
    }
    connections {
        Frame::<2>(a.output, b.output) -> filters.input;
        filters[0].output -> out;
    }
}

fn main() {}
