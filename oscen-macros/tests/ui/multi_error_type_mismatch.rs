use oscen::graph;

graph! {
    name: TypeMismatchTwice;

    input stream s1;
    input stream s2;
    input value v1;

    output value v_out;

    connections {
        s1 -> v_out;
        v1 -> v_out;
        s2 -> v_out;
    }
}

fn main() {}
