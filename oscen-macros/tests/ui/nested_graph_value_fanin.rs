// Two drivers into a nested graph's value input: the parent cannot see the
// child's endpoint kinds, so rustc must reject it through the child graph's
// `EndpointAt` marker with the `FanInAllowed` diagnostic (not an
// "associated type not found" error, and not a silent sum).

use oscen::{graph, PolyBlepOscillator};

graph! {
    name: Inner;
    input value level;
    output stream out;
    connections { level -> out; }
}

graph! {
    name: Outer;
    output stream out;
    nodes {
        a = PolyBlepOscillator::sine(1.0, 1.0);
        b = PolyBlepOscillator::sine(2.0, 1.0);
        inner = Inner::new();
    }
    connections {
        a.output -> inner.level;
        b.output -> inner.level;
        inner.out -> out;
    }
}

fn main() {}
