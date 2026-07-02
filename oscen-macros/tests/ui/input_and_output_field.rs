use oscen::graph::EventInput;
use oscen::Node;

#[derive(Debug, Default, Node)]
struct BadNode {
    #[input(event)]
    #[output(event)]
    pub ev: EventInput,
}

fn main() {}
