/// Headless test that mimics supersaw's structure
/// Tests JIT with stream/value connections (no events)
use oscen::{Graph, InputEndpoint, Node, NodeKey, PolyBlepOscillator, ProcessingContext, ProcessingNode, SignalProcessor, TptFilter, ValueKey};
use oscen::graph::jit::{CraneliftJit, GraphStateBuilder};

const NUM_OSCILLATORS: usize = 5;
const DETUNE_OFFSETS: [f32; NUM_OSCILLATORS] = [-4.0, -2.0, 0.0, 2.0, 4.0];
const DETUNE_STEP_CENTS: f32 = 300.0;

#[derive(Debug, Node)]
struct DetuneFrequency {
    #[input(value)]
    base_frequency: f32,
    #[input(value)]
    spread: f32,

    #[output(value)]
    frequency: f32,

    offset_steps: f32,
}

impl DetuneFrequency {
    fn new(offset_steps: f32) -> Self {
        Self {
            base_frequency: 0.0,
            spread: 0.0,
            frequency: 0.0,
            offset_steps,
        }
    }
}

impl SignalProcessor for DetuneFrequency {
    fn process<'a>(&mut self, _sample_rate: f32, context: &mut ProcessingContext<'a>) -> f32 {
        let base = self.get_base_frequency(context).max(0.0);
        let spread = self.get_spread(context).clamp(0.0, 1.0);
        let cents = self.offset_steps * spread * DETUNE_STEP_CENTS;
        let ratio = 2f32.powf(cents / 1200.0);
        self.frequency = base * ratio;
        self.frequency
    }
}

#[test]
fn test_jit_supersaw_structure() {
    let sample_rate = 44100.0;

    println!("\n=== Testing Supersaw Structure with Interpreted Mode ===");

    // Build a graph similar to supersaw
    let mut graph_interp = Graph::new(sample_rate);

    let base_param = graph_interp.value_param(440.0);
    let spread_param = graph_interp.value_param(0.5); // 50% spread
    let cutoff_param = graph_interp.value_param(3000.0);
    let q_param = graph_interp.value_param(0.707);
    let volume_param = graph_interp.value_param(0.4);

    let mut summed_osc_output = None;
    let osc_amplitude = 1.0 / NUM_OSCILLATORS as f32;

    for &offset_steps in DETUNE_OFFSETS.iter() {
        let detune = graph_interp.add_node(DetuneFrequency::new(offset_steps));
        let osc = graph_interp.add_node(PolyBlepOscillator::saw(440.0, osc_amplitude));

        graph_interp.connect_all(vec![
            base_param >> detune.base_frequency,
            spread_param >> detune.spread,
            detune.frequency >> osc.frequency,
        ]);

        let osc_output = osc.output;
        summed_osc_output = Some(match summed_osc_output {
            Some(accum) => graph_interp.combine(accum, osc_output, |a, b| a + b),
            None => osc_output,
        });
    }

    let filter = graph_interp.add_node(TptFilter::new(3000.0, 0.707));
    let summed_osc_output = summed_osc_output.expect("No oscillators were created");

    graph_interp.connect_all(vec![
        cutoff_param >> filter.cutoff,
        q_param >> filter.q,
        summed_osc_output >> filter.input,
    ]);

    let output = graph_interp.combine(filter.output, volume_param, |x, v| x * v);

    // Process 10 frames with interpreted mode
    let mut interpreted_outputs = Vec::new();
    for i in 0..10 {
        graph_interp.process().expect("Process failed");
        let value = graph_interp.get_value(&output).unwrap_or(0.0);

        // Debug: Check oscillator frequency on first frame
        if i == 1 {
            // Node 5 should be first oscillator - check its frequency input
            println!("\n=== INTERPRETED Mode - Frame 1 ===");
        }

        interpreted_outputs.push(value);
    }

    println!("Interpreted outputs (first 5): {:?}", &interpreted_outputs[..5]);

    // Now test with JIT
    println!("\n=== Testing Supersaw Structure with JIT Mode ===");

    let mut graph_jit = Graph::new(sample_rate);

    let base_param2 = graph_jit.value_param(440.0);
    let spread_param2 = graph_jit.value_param(0.5);
    let cutoff_param2 = graph_jit.value_param(3000.0);
    let q_param2 = graph_jit.value_param(0.707);
    let volume_param2 = graph_jit.value_param(0.4);

    let mut summed_osc_output2 = None;

    for &offset_steps in DETUNE_OFFSETS.iter() {
        let detune = graph_jit.add_node(DetuneFrequency::new(offset_steps));
        let osc = graph_jit.add_node(PolyBlepOscillator::saw(440.0, osc_amplitude));

        graph_jit.connect_all(vec![
            base_param2 >> detune.base_frequency,
            spread_param2 >> detune.spread,
            detune.frequency >> osc.frequency,
        ]);

        let osc_output = osc.output;
        summed_osc_output2 = Some(match summed_osc_output2 {
            Some(accum) => graph_jit.combine(accum, osc_output, |a, b| a + b),
            None => osc_output,
        });
    }

    let filter2 = graph_jit.add_node(TptFilter::new(3000.0, 0.707));
    let summed_osc_output2 = summed_osc_output2.expect("No oscillators were created");

    graph_jit.connect_all(vec![
        cutoff_param2 >> filter2.cutoff,
        q_param2 >> filter2.q,
        summed_osc_output2 >> filter2.input,
    ]);

    let output2 = graph_jit.combine(filter2.output, volume_param2, |x, v| x * v);

    // JIT compile
    let ir = graph_jit.to_ir().expect("Failed to extract IR");
    println!("Extracted IR with {} nodes", ir.nodes.len());

    let mut jit = CraneliftJit::new().expect("Failed to create JIT");
    let compiled = jit.compile(&ir).expect("Failed to compile");
    println!("Successfully compiled graph");

    let mut state_builder = GraphStateBuilder::new(&ir, &mut graph_jit.nodes);

    // Process 10 frames with JIT
    let mut jit_outputs = Vec::new();
    for _ in 0..10 {
        graph_jit.process_ramps();

        let (mut state, _temps) = state_builder.build(
            &mut graph_jit.nodes,
            &mut graph_jit.endpoints,
        );

        let _ = compiled.process(&mut state);

        let value = graph_jit.get_value(&output2).unwrap_or(0.0);
        jit_outputs.push(value);
    }

    println!("JIT outputs (first 5): {:?}", &jit_outputs[..5]);

    // Compare results
    println!("\n=== Comparison ===");
    let mut max_diff = 0.0f32;
    let mut non_zero_count_interp = 0;
    let mut non_zero_count_jit = 0;

    for (i, (&interp, &jit)) in interpreted_outputs.iter().zip(jit_outputs.iter()).enumerate() {
        let diff = (interp - jit).abs();
        max_diff = max_diff.max(diff);

        if interp.abs() > 0.0001 {
            non_zero_count_interp += 1;
        }
        if jit.abs() > 0.0001 {
            non_zero_count_jit += 1;
        }

        println!("Frame {}: Interpreted={:.6}, JIT={:.6}, Diff={:.6}", i, interp, jit, diff);
    }

    println!("\nMax difference: {:.6}", max_diff);
    println!("Non-zero frames - Interpreted: {}/10, JIT: {}/10", non_zero_count_interp, non_zero_count_jit);

    // The outputs should be very similar (allow for small floating point differences)
    assert!(max_diff < 0.01, "JIT output differs too much from interpreted: {}", max_diff);
    assert!(non_zero_count_jit > 0, "JIT produced no non-zero output!");

    println!("\n✓ Supersaw JIT test PASSED - outputs match within tolerance");
}
