use oscen::graph;
use oscen::SignalProcessor;

// Simple node with const generic
#[derive(Clone, Copy, Debug, Default)]
pub struct GenericNode<const N: usize> {
    pub val: f32,
}

impl<const N: usize> GenericNode<N> {
    pub fn new() -> Self {
        Self::default()
    }

    // Required for static graphs - called before process()
    pub fn process_event_inputs(&mut self) {}
}

impl<const N: usize> SignalProcessor for GenericNode<N> {
    fn init(&mut self, _sample_rate: f32) {}
    fn process(&mut self) {}
}

graph! {
    name: TurbofishTest;

    input value test_input = 1.0;

    nodes {
        generic_node = GenericNode::<4>;
    }

    connections {
        test_input -> generic_node.val;
    }
}

fn main() {
    let mut graph = TurbofishTest::new();
    graph.init(44100.0);
    graph.test_input = 5.0;
    graph.process();

    println!("Generic node val: {}", graph.generic_node.val);
    assert_eq!(graph.generic_node.val, 5.0);

    println!("Test passed!");
}
