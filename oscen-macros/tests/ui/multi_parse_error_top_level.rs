use oscen::graph;

graph! {
    name: TwoBadItems;

    input stream s1
    input stream s2;
    output stream out;

    foo bar baz;

    connections {
        s1 -> out;
        s2 -> out;
    }
}

fn main() {}
