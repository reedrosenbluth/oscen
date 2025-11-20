use oscen::graph;

// Simple node with const generic
#[derive(Clone, Copy, Debug)]
pub struct GenericNode<const N: usize> {
    pub val: f32,
}

impl<const N: usize> GenericNode<N> {
    pub fn new(_sample_rate: f32) -> Self {
        Self { val: 0.0 }
    }
    pub fn process(&mut self) {}
}

graph! {
    name: TurbofishTest;
    compile_time: true;

    input value test_input = 1.0;

    nodes {
        generic_node = GenericNode::<4>;
    }

    connections {
        test_input -> generic_node.val;
    }
}

fn main() {
    let mut graph = TurbofishTest::new(44100.0);
    graph.test_input = 5.0;
    graph.process();

    println!("Generic node val: {}", graph.generic_node.val);
    assert_eq!(graph.generic_node.val, 5.0);

    println!("Test passed!");
}
