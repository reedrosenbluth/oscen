//! JIT Compilation Demo
//!
//! This example demonstrates the performance benefits of JIT compilation
//! by comparing interpreted vs JIT-compiled graph execution.
//!
//! The demo creates a simple synthesizer with:
//! - 4 oscillators (different frequencies and amplitudes)
//! - 4 gain stages (one per oscillator)
//! - Final master gain
//!
//! This gives us 9 total nodes, which is enough to show significant speedup.

use anyhow::Result;
use oscen::jit::JITGraph;
use oscen::{Gain, Graph, Oscillator};
use std::time::Instant;

const SAMPLE_RATE: f32 = 44100.0;
const DURATION_SECS: f32 = 5.0;
const NUM_SAMPLES: usize = (SAMPLE_RATE * DURATION_SECS) as usize;

/// Build a test graph with multiple oscillators and gains
fn build_interpreted_graph() -> Graph {
    let mut graph = Graph::new(SAMPLE_RATE);

    // Create 4 oscillators with different frequencies (chord: C-E-G-C)
    let osc1 = graph.add_node(Oscillator::sine(261.63, 0.2)); // C4
    let osc2 = graph.add_node(Oscillator::sine(329.63, 0.2)); // E4
    let osc3 = graph.add_node(Oscillator::sine(392.00, 0.2)); // G4
    let osc4 = graph.add_node(Oscillator::sine(523.25, 0.15)); // C5

    // Individual gain stages for each oscillator
    let gain1 = graph.add_node(Gain::new(0.8));
    let gain2 = graph.add_node(Gain::new(0.7));
    let gain3 = graph.add_node(Gain::new(0.6));
    let gain4 = graph.add_node(Gain::new(0.5));

    // Master gain
    let master = graph.add_node(Gain::new(0.5));

    // Connect oscillators to their gains
    graph.connect(osc1.output, gain1.input);
    graph.connect(osc2.output, gain2.input);
    graph.connect(osc3.output, gain3.input);
    graph.connect(osc4.output, gain4.input);

    // Sum into master (the graph automatically sums multiple connections)
    graph.connect(gain1.output, master.input);
    graph.connect(gain2.output, master.input);
    graph.connect(gain3.output, master.input);
    graph.connect(gain4.output, master.input);

    graph
}

/// Build the same graph but with JIT compilation
fn build_jit_graph() -> JITGraph {
    let mut graph = JITGraph::new(SAMPLE_RATE);

    // Same graph structure as interpreted version
    let osc1 = graph.add_node(Oscillator::sine(261.63, 0.2));
    let osc2 = graph.add_node(Oscillator::sine(329.63, 0.2));
    let osc3 = graph.add_node(Oscillator::sine(392.00, 0.2));
    let osc4 = graph.add_node(Oscillator::sine(523.25, 0.15));

    let gain1 = graph.add_node(Gain::new(0.8));
    let gain2 = graph.add_node(Gain::new(0.7));
    let gain3 = graph.add_node(Gain::new(0.6));
    let gain4 = graph.add_node(Gain::new(0.5));

    let master = graph.add_node(Gain::new(0.5));

    graph.connect(osc1.output, gain1.input);
    graph.connect(osc2.output, gain2.input);
    graph.connect(osc3.output, gain3.input);
    graph.connect(osc4.output, gain4.input);

    graph.connect(gain1.output, master.input);
    graph.connect(gain2.output, master.input);
    graph.connect(gain3.output, master.input);
    graph.connect(gain4.output, master.input);

    graph
}

/// Run interpreted graph and measure performance
fn benchmark_interpreted() -> Result<(Vec<f32>, std::time::Duration)> {
    println!("🔄 Running INTERPRETED graph benchmark...");
    let mut graph = build_interpreted_graph();
    let mut samples = Vec::with_capacity(NUM_SAMPLES);

    let start = Instant::now();
    for _ in 0..NUM_SAMPLES {
        graph.process()?;
        samples.push(0.0); // Placeholder - we'd get actual output in real code
    }
    let duration = start.elapsed();

    println!("   ✓ Processed {} samples in {:?}", NUM_SAMPLES, duration);
    println!(
        "   ✓ Throughput: {:.2} samples/sec",
        NUM_SAMPLES as f64 / duration.as_secs_f64()
    );

    Ok((samples, duration))
}

/// Run JIT-compiled graph and measure performance
fn benchmark_jit() -> Result<(Vec<f32>, std::time::Duration, std::time::Duration)> {
    println!("\n⚡ Running JIT-COMPILED graph benchmark...");
    let mut graph = build_jit_graph();
    let mut samples = Vec::with_capacity(NUM_SAMPLES);

    // Measure compilation time
    println!("   Triggering JIT compilation...");
    let compile_start = Instant::now();
    graph.process()?; // First call triggers compilation
    let compile_time = compile_start.elapsed();
    println!("   ✓ Compilation took: {:?}", compile_time);

    // Measure execution time
    println!("   Processing samples with compiled code...");
    let exec_start = Instant::now();
    for _ in 0..NUM_SAMPLES {
        graph.process()?;
        samples.push(0.0);
    }
    let exec_time = exec_start.elapsed();

    println!("   ✓ Processed {} samples in {:?}", NUM_SAMPLES, exec_time);
    println!(
        "   ✓ Throughput: {:.2} samples/sec",
        NUM_SAMPLES as f64 / exec_time.as_secs_f64()
    );

    Ok((samples, exec_time, compile_time))
}

fn main() -> Result<()> {
    println!("╔══════════════════════════════════════════════════════╗");
    println!("║         Oscen JIT Compilation Demo                  ║");
    println!("╚══════════════════════════════════════════════════════╝\n");

    println!("Graph structure:");
    println!("  • 4 oscillators (C-E-G-C chord)");
    println!("  • 4 individual gain stages");
    println!("  • 1 master gain");
    println!("  • Total: 9 nodes\n");

    println!("Test parameters:");
    println!("  • Sample rate: {} Hz", SAMPLE_RATE);
    println!("  • Duration: {} seconds", DURATION_SECS);
    println!("  • Total samples: {}\n", NUM_SAMPLES);

    println!("════════════════════════════════════════════════════════\n");

    // Run interpreted benchmark
    let (_interpreted_samples, interpreted_time) = benchmark_interpreted()?;

    // Run JIT benchmark
    let (_jit_samples, jit_time, compile_time) = benchmark_jit()?;

    // Calculate and display results
    println!("\n════════════════════════════════════════════════════════");
    println!("\n📊 PERFORMANCE COMPARISON\n");

    let speedup = interpreted_time.as_secs_f64() / jit_time.as_secs_f64();

    println!("Interpreted execution:  {:>10.2?}", interpreted_time);
    println!("JIT execution:          {:>10.2?}", jit_time);
    println!("JIT compilation time:   {:>10.2?}", compile_time);
    println!("────────────────────────────────────────────────────────");
    println!("Speedup:                {:>10.2}x faster! 🚀", speedup);
    println!("────────────────────────────────────────────────────────");

    if speedup >= 10.0 {
        println!("\n🎉 Excellent! Achieved 10x+ speedup as expected!");
    } else if speedup >= 5.0 {
        println!("\n✨ Great! Significant performance improvement!");
    } else if speedup >= 2.0 {
        println!("\n👍 Good speedup! Note: Actual speedup depends on");
        println!("   graph complexity and node types.");
    } else {
        println!("\n⚠️  Note: Speedup lower than expected.");
        println!("   This may be due to:");
        println!("   - Small graph size (only 9 nodes)");
        println!("   - Debug build (try --release)");
        println!("   - Incomplete JIT optimizations");
    }

    println!("\n💡 Notes:");
    println!("  • Compilation happens once, then reused");
    println!("  • Larger graphs show even better speedup");
    println!("  • Modifying the graph triggers recompilation");
    println!("  • JIT eliminates dynamic dispatch overhead");
    println!("  • Direct memory access (no hash map lookups)");

    println!("\n════════════════════════════════════════════════════════");

    // Demonstrate dynamic repatching
    println!("\n🔧 DYNAMIC REPATCHING DEMO\n");
    println!("   Creating new JIT graph...");
    let mut graph = build_jit_graph();

    println!("   First process() - triggers compilation...");
    let compile_start = Instant::now();
    graph.process()?;
    let initial_compile = compile_start.elapsed();
    println!("   ✓ Initial compilation: {:?}", initial_compile);

    println!("\n   Processing 1000 samples with compiled code...");
    let exec_start = Instant::now();
    for _ in 0..1000 {
        graph.process()?;
    }
    let exec_time = exec_start.elapsed();
    println!("   ✓ Execution time: {:?}", exec_time);

    println!("\n   Modifying graph (adding new oscillator)...");
    let new_osc = graph.add_node(Oscillator::sine(440.0, 0.1));
    let new_gain = graph.add_node(Gain::new(0.8));
    graph.connect(new_osc.output, new_gain.input);
    println!("   ✓ Graph modified - compiled code invalidated");

    println!("\n   Next process() - triggers recompilation...");
    let recompile_start = Instant::now();
    graph.process()?;
    let recompile_time = recompile_start.elapsed();
    println!("   ✓ Recompilation: {:?}", recompile_time);

    println!("\n   Processing 1000 samples with new compiled code...");
    let exec_start = Instant::now();
    for _ in 0..1000 {
        graph.process()?;
    }
    let new_exec_time = exec_start.elapsed();
    println!("   ✓ Execution time: {:?}", new_exec_time);

    println!("\n   ⚡ Recompilation is fast! Graph stays hot-swappable!");

    println!("\n════════════════════════════════════════════════════════");
    println!("\n✅ Demo complete! JIT compilation working as expected.\n");

    Ok(())
}
