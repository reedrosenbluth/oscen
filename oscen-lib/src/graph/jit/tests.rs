/// Tests for JIT compilation

#[cfg(test)]
mod tests {
    use crate::graph::{Graph, jit::{CraneliftJit, GraphStateBuilder}};
    use crate::gain::Gain;

    #[test]
    fn test_jit_simple_gain() {
        // Create a simple graph with one gain node
        let mut graph = Graph::new(44100.0);

        let gain_node = graph.add_node(Gain::new(2.0));

        // Set the stream input value directly (since stream inputs can't use set_value)
        use crate::graph::types::ValueKey;
        let input_key: ValueKey = gain_node.input.into();
        if let Some(endpoint) = graph.endpoints.get_mut(input_key) {
            endpoint.set_scalar(0.5);
        }

        // Extract IR
        let ir = graph.to_ir().expect("Failed to extract IR");
        assert_eq!(ir.nodes.len(), 1, "Should have 1 node");

        // Compile
        let mut jit = CraneliftJit::new().expect("Failed to create JIT");
        let compiled = jit.compile(&ir).expect("Failed to compile");

        // Build state
        let mut state_builder = GraphStateBuilder::new(&ir, &mut graph.nodes);
        let (mut state, _temps) = state_builder.build(&mut graph.nodes, &mut graph.endpoints);

        // Execute!
        let output = compiled.process(&mut state);

        // Gain of 2.0 * input of 0.5 = 1.0
        assert!((output - 1.0).abs() < 0.001, "Expected 1.0, got {}", output);
    }

    #[test]
    fn test_jit_chain() {
        // Create a graph with two gains chained: input -> gain1(2.0) -> gain2(3.0)
        let mut graph = Graph::new(44100.0);

        let gain1 = graph.add_node(Gain::new(2.0));
        let gain2 = graph.add_node(Gain::new(3.0));

        // Connect them
        graph.connect(gain1.output, gain2.input);

        // Set the initial input directly (stream inputs can't use set_value)
        use crate::graph::types::ValueKey;
        let input_key: ValueKey = gain1.input.into();
        if let Some(endpoint) = graph.endpoints.get_mut(input_key) {
            endpoint.set_scalar(0.5);
        }

        // Extract IR
        let ir = graph.to_ir().expect("Failed to extract IR");
        assert_eq!(ir.nodes.len(), 2, "Should have 2 nodes");

        // Compile
        let mut jit = CraneliftJit::new().expect("Failed to create JIT");
        let compiled = jit.compile(&ir).expect("Failed to compile");

        // Build state
        let mut state_builder = GraphStateBuilder::new(&ir, &mut graph.nodes);
        let (mut state, _temps) = state_builder.build(&mut graph.nodes, &mut graph.endpoints);

        // Execute!
        let output = compiled.process(&mut state);

        // 0.5 * 2.0 * 3.0 = 3.0
        assert!((output - 3.0).abs() < 0.001, "Expected 3.0, got {}", output);
    }

    #[test]
    fn test_jit_compilation_basic() {
        // Just test that the JIT compiler can be created and compile an empty-ish graph
        let mut graph = Graph::new(44100.0);
        let gain = graph.add_node(Gain::new(1.0));

        let ir = graph.to_ir().expect("Failed to extract IR");
        let mut jit = CraneliftJit::new().expect("Failed to create JIT");
        let _compiled = jit.compile(&ir).expect("Failed to compile");

        // If we got here, compilation succeeded!
    }
}
