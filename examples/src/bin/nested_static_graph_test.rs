use oscen::graph;

// Inner static graph
graph! {
    name: InnerGraph;
    compile_time: true;

    input value inner_input = 1.0;
    
    nodes {
        // Just a dummy node to verify structure
        dummy = DummyNode;
    }

    connections {
        inner_input -> dummy.val;
    }
}

#[derive(Clone, Copy, Debug)]
pub struct DummyNode {
    pub val: f32,
}

impl DummyNode {
    pub fn new(_sample_rate: f32) -> Self {
        Self { val: 0.0 }
    }
    pub fn process(&mut self) {}
}

// Outer static graph using InnerGraph
graph! {
    name: OuterGraph;
    compile_time: true;

    input value outer_input = 10.0;

    nodes {
        // Nested graph as a node
        inner = InnerGraph;
        // Array of nested graphs
        inner_array = [InnerGraph; 2];
    }

    connections {
        outer_input -> inner.inner_input;
        outer_input -> inner_array.inner_input;
    }
}

fn main() {
    let mut graph = OuterGraph::new(44100.0);
    
    graph.outer_input = 5.0;
    graph.process();
    
    // Verify propagation
    // Outer input (5.0) -> Inner input -> Dummy node val
    
    println!("Inner dummy val: {}", graph.inner.dummy.val);
    println!("Inner array[0] dummy val: {}", graph.inner_array[0].dummy.val);
    println!("Inner array[1] dummy val: {}", graph.inner_array[1].dummy.val);

    assert_eq!(graph.inner.dummy.val, 5.0);
    assert_eq!(graph.inner_array[0].dummy.val, 5.0);
    assert_eq!(graph.inner_array[1].dummy.val, 5.0);
    
    println!("Nested graph test passed!");
}
