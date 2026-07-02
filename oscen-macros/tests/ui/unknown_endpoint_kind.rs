use oscen::Node;

#[derive(Debug, Default, Node)]
struct BadNode {
    #[input(strem)]
    pub input: f32,
}

fn main() {}
