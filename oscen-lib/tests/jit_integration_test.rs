// Integration test for JIT compilation
// This tests that JIT produces the same output as interpreted execution

use oscen::Gain;
use oscen::graph::Graph;
use oscen::graph::jit::{CraneliftJit, GraphStateBuilder};
use oscen::graph::types::ValueKey;

#[test]
fn test_jit_vs_interpreted() {
    let sample_rate = 44100.0;

    println!("\n=== Testing Interpreted Execution ===");
    let mut graph_interp = Graph::new(sample_rate);
    let gain1 = graph_interp.add_node(Gain::new(2.0));

    // Set the input directly (Gain has a stream input)
    let input_key: ValueKey = gain1.input.into();
    if let Some(endpoint) = graph_interp.endpoints.get_mut(input_key) {
        endpoint.set_scalar(3.0);
    }

    let mut interpreted_outputs = Vec::new();
    for i in 0..5 {
        graph_interp.process().expect("Process failed");
        let output_key = gain1.output.key();
        let output = graph_interp.endpoints
            .get(output_key)
            .and_then(|ep| ep.as_scalar())
            .unwrap_or(0.0);
        println!("Interpreted Frame {}: input=3.0, gain=2.0, output={}", i, output);
        interpreted_outputs.push(output);
    }

    println!("\n=== Testing JIT Execution ===");
    let mut graph_jit = Graph::new(sample_rate);
    let gain2 = graph_jit.add_node(Gain::new(2.0));

    // Set the input directly
    let input_key: ValueKey = gain2.input.into();
    if let Some(endpoint) = graph_jit.endpoints.get_mut(input_key) {
        endpoint.set_scalar(3.0);
    }

    let ir = graph_jit.to_ir().expect("Failed to extract IR");
    println!("Extracted IR with {} nodes", ir.nodes.len());

    let mut jit = CraneliftJit::new().expect("Failed to create JIT");
    let compiled = jit.compile(&ir).expect("Failed to compile");
    println!("Successfully compiled graph");

    let mut state_builder = GraphStateBuilder::new(&ir, &mut graph_jit.nodes);

    let mut jit_outputs = Vec::new();
    for i in 0..5 {
        let (mut state, _temps) = state_builder.build(
            &mut graph_jit.nodes,
            &mut graph_jit.endpoints,
        );

        compiled.process(&mut state);

        let output_key = gain2.output.key();
        let output = graph_jit.endpoints
            .get(output_key)
            .and_then(|ep| ep.as_scalar())
            .unwrap_or(0.0);
        println!("JIT Frame {}: input=3.0, gain=2.0, output={}", i, output);
        jit_outputs.push(output);
    }

    println!("\n=== Comparing Outputs ===");
    for (i, (interp, jit)) in interpreted_outputs.iter().zip(jit_outputs.iter()).enumerate() {
        println!("Frame {}: interpreted={}, jit={}, match={}", i, interp, jit, (interp - jit).abs() < 0.0001);
        assert!((interp - jit).abs() < 0.0001, "Frame {} mismatch: interpreted={}, jit={}", i, interp, jit);
    }

    println!("All outputs match! Expected 6.0 (3.0 * 2.0)");
    assert!((interpreted_outputs[0] - 6.0).abs() < 0.0001, "Expected output 6.0, got {}", interpreted_outputs[0]);
}
