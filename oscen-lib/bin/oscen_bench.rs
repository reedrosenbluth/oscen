// Simple oscen benchmark runner matching JUCE format
// Run with: cargo run --release --bin oscen_bench

use oscen::{AdsrEnvelope, Delay, Graph, Oscillator, PolyBlepOscillator, TptFilter};
use std::time::Instant;

fn simple_graph() -> Graph {
    let mut graph = Graph::new(44100.0);
    let _osc = graph.add_node(Oscillator::sine(440.0, 1.0));
    graph
}

fn medium_graph() -> Graph {
    let mut graph = Graph::new(44100.0);

    // Medium: 2 oscillators + filter + envelope
    let osc1 = graph.add_node(Oscillator::sine(440.0, 1.0));
    let osc2 = graph.add_node(PolyBlepOscillator::saw(442.0, 1.0));
    let filter = graph.add_node(TptFilter::new(1000.0, 0.7));
    let env = graph.add_node(AdsrEnvelope::new(0.01, 0.1, 0.7, 0.2));

    let mixed = graph.add(osc1.output, osc2.output);
    graph.connect(mixed, filter.input);

    let _final_out = graph.multiply(filter.output, env.output);

    graph
}

fn complex_graph() -> Graph {
    let mut graph = Graph::new(44100.0);

    // Complex: 5 oscillators + 2 filters + 2 envelopes + delay
    let osc1 = graph.add_node(Oscillator::sine(440.0, 0.3));
    let osc2 = graph.add_node(PolyBlepOscillator::saw(450.0, 0.3));
    let osc3 = graph.add_node(Oscillator::sine(460.0, 0.3));
    let osc4 = graph.add_node(PolyBlepOscillator::saw(470.0, 0.3));
    let osc5 = graph.add_node(Oscillator::sine(480.0, 0.3));

    // Mix first 3 oscillators
    let mix1 = graph.add(osc1.output, osc2.output);
    let mix2 = graph.add(mix1, osc3.output);

    // Mix last 2 oscillators
    let mix3 = graph.add(osc4.output, osc5.output);

    // Filter each mix
    let filter1 = graph.add_node(TptFilter::new(800.0, 0.5));
    let filter2 = graph.add_node(TptFilter::new(1200.0, 0.5));

    graph.connect(mix2, filter1.input);
    graph.connect(mix3, filter2.input);

    // Envelopes
    let env1 = graph.add_node(AdsrEnvelope::new(0.01, 0.1, 0.7, 0.2));
    let env2 = graph.add_node(AdsrEnvelope::new(0.02, 0.15, 0.6, 0.3));

    // Apply envelopes
    let filtered1 = graph.multiply(filter1.output, env1.output);
    let filtered2 = graph.multiply(filter2.output, env2.output);

    // Mix and delay
    let final_mix = graph.add(filtered1, filtered2);
    let delay = graph.add_node(Delay::from_seconds(0.5, 0.3, 44100.0));

    graph.connect(final_mix, delay.input);

    graph
}

fn run_benchmark(name: &str, mut graph: Graph) {
    let num_samples = 441_000;
    let sample_rate = 44100.0;

    graph.validate().unwrap();

    println!("=== Oscen {} ===", name);
    println!("Processing {} samples...", num_samples);

    let start = Instant::now();

    for _ in 0..num_samples {
        graph.process().unwrap();
    }

    let elapsed = start.elapsed();
    let elapsed_micros = elapsed.as_micros();
    let elapsed_secs = elapsed.as_secs_f64();

    let samples_per_second = num_samples as f64 / elapsed_secs;
    let real_time_factor = (num_samples as f64 / sample_rate) / elapsed_secs;
    let micros_per_sample = elapsed_micros as f64 / num_samples as f64;

    println!("Processed {} samples in {} microseconds", num_samples, elapsed_micros);
    println!("Samples per second: {:.2}", samples_per_second);
    println!("Real-time factor: {:.2}x", real_time_factor);
    println!("Microseconds per sample: {:.2}", micros_per_sample);
    println!();
}

fn main() {
    run_benchmark("Simple Graph (1 oscillator)", simple_graph());
    run_benchmark("Medium Graph (2 osc + filter + env)", medium_graph());
    run_benchmark("Complex Graph (5 osc + 2 filters + 2 env + delay)", complex_graph());
}
