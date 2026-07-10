use oscen::{graph, Gain};

graph! {
    name: BadArrays;
    input stream s;
    output stream out;
    nodes {
        a = [Gain::new(0.5); 4];
        b = [Gain::new(0.5); 2];
    }
    connections {
        s -> a.input;
        a.output -> b.input;  // 4 -> 2: silently truncated before, now an error
        b[0].output -> out;
    }
}

fn main() {}
