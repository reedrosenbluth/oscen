use oscen::{graph, PolyBlepOscillator, TptFilter};

// Two sources fan into one stream input, but one is a compound (arithmetic)
// source. Auto-summing fan-in supports only same-rate simple scalar/frame
// stream sources, so codegen must emit a scoped `compile_error!` rather than
// silently producing wrong audio.
graph! {
    name: BadFanin;
    input value gain = 0.5;
    output stream out;
    nodes {
        osc = PolyBlepOscillator::saw(440.0, 0.6);
        osc2 = PolyBlepOscillator::saw(220.0, 0.6);
        filter = TptFilter::new(800.0, 0.7);
    }
    connections {
        osc.output * gain -> filter.input;
        osc2.output -> filter.input;
        filter.output -> out;
    }
}

fn main() {}
