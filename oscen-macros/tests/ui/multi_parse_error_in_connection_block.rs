use oscen::graph;

graph! {
    name: BadConnectionBlock;

    input stream s1;
    input stream s2;
    input stream s3;
    output stream out;

    connections {
        s1 ~> out;
        s2 -> out;
        s3 -> ;
    }
}

fn main() {}
