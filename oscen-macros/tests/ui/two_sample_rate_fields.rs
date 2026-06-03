use oscen::{Node, SampleRate};

#[derive(Debug, Node)]
struct TwoRates {
    a: SampleRate,
    b: SampleRate,
}

fn main() {}
