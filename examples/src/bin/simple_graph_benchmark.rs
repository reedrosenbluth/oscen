/// Simple benchmark comparing static vs runtime graph performance
/// Demonstrates the GraphInterface trait without complex array features

use oscen::prelude::*;
use oscen::graph::GraphInterface;
use std::time::{Duration, Instant};

// Static version (compile_time: true)
graph! {
    name: SimpleFilterStatic;
    compile_time: true;

    input cutoff: value = 1000.0;
    input resonance: value = 0.707;

    output out: stream;

    nodes {
        osc = PolyBlepOscillator::saw(440.0, 0.5);
        filter = TptFilter::new(1000.0, 0.707);
    }

    connections {
        osc.output -> filter.input;
        cutoff -> filter.cutoff;
        resonance -> filter.q;
        filter.output -> out;
    }
}

// Runtime version (compile_time: false)
graph! {
    name: SimpleFilterRuntime;
    compile_time: false;

    input cutoff: value = 1000.0;
    input resonance: value = 0.707;

    output out: stream;

    nodes {
        osc = PolyBlepOscillator::saw(440.0, 0.5);
        filter = TptFilter::new(1000.0, 0.707);
    }

    connections {
        osc.output -> filter.input;
        cutoff -> filter.cutoff;
        resonance -> filter.q;
        filter.output -> out;
    }
}

struct BenchmarkStats {
    total_time: Duration,
    min_time: Duration,
    max_time: Duration,
    sample_count: usize,
}

impl BenchmarkStats {
    fn new() -> Self {
        Self {
            total_time: Duration::ZERO,
            min_time: Duration::from_secs(u64::MAX),
            max_time: Duration::ZERO,
            sample_count: 0,
        }
    }

    fn record(&mut self, elapsed: Duration) {
        self.total_time += elapsed;
        self.min_time = self.min_time.min(elapsed);
        self.max_time = self.max_time.max(elapsed);
        self.sample_count += 1;
    }

    fn avg_micros(&self) -> f64 {
        if self.sample_count == 0 {
            return 0.0;
        }
        self.total_time.as_nanos() as f64 / self.sample_count as f64 / 1000.0
    }

    fn min_micros(&self) -> f64 {
        self.min_time.as_nanos() as f64 / 1000.0
    }

    fn max_micros(&self) -> f64 {
        self.max_time.as_nanos() as f64 / 1000.0
    }
}

/// Run benchmark using the unified GraphInterface API
/// This function is mode-agnostic - works with both static and runtime graphs!
fn benchmark_graph<G: GraphInterface>(mut graph: G, name: &str, samples: usize) -> BenchmarkStats {
    let mut stats = BenchmarkStats::new();

    // Use unified API to set parameters
    graph.set_input_value("cutoff", 2000.0);
    graph.set_input_value("resonance", 0.8);

    println!("Benchmarking {} ({} samples)...", name, samples);

    // Warm up
    for _ in 0..1000 {
        graph.process_sample();
    }

    // Benchmark
    for _ in 0..samples {
        let start = Instant::now();
        let _output = graph.process_sample();
        stats.record(start.elapsed());
    }

    stats
}

fn main() {
    const SAMPLE_RATE: f32 = 48_000.0;
    const BENCHMARK_SAMPLES: usize = 100_000;

    println!("\n🎛️  Graph Mode Performance Benchmark");
    println!("===================================\n");
    println!("Testing simple filtered oscillator:");
    println!("  - PolyBlepOscillator (sawtooth)");
    println!("  - TptFilter (state-variable)\n");

    // Benchmark static graph
    let static_graph = SimpleFilterStatic::new(SAMPLE_RATE);
    let static_stats = benchmark_graph(static_graph, "Static Graph (compile_time: true)", BENCHMARK_SAMPLES);

    // Benchmark runtime graph
    let runtime_graph = SimpleFilterRuntime::new(SAMPLE_RATE);
    let runtime_stats = benchmark_graph(runtime_graph, "Runtime Graph (compile_time: false)", BENCHMARK_SAMPLES);

    // Print results
    println!("\n📊 Results:");
    println!("==========\n");

    println!("Static Graph (compile_time: true):");
    println!("  Average: {:.3} µs/sample", static_stats.avg_micros());
    println!("  Min:     {:.3} µs", static_stats.min_micros());
    println!("  Max:     {:.3} µs", static_stats.max_micros());

    println!("\nRuntime Graph (compile_time: false):");
    println!("  Average: {:.3} µs/sample", runtime_stats.avg_micros());
    println!("  Min:     {:.3} µs", runtime_stats.min_micros());
    println!("  Max:     {:.3} µs", runtime_stats.max_micros());

    let ratio = runtime_stats.avg_micros() / static_stats.avg_micros();
    println!("\n⚡ Performance Ratio:");
    println!("  Runtime / Static = {:.2}x", ratio);

    if ratio < 1.5 {
        println!("  ✅ Runtime performance is excellent! (< 1.5x overhead)");
    } else if ratio < 3.0 {
        println!("  ✓ Runtime performance is good (< 3x overhead)");
    } else {
        println!("  ⚠ Runtime has significant overhead (> 3x)");
    }

    // Calculate throughput
    let static_throughput = SAMPLE_RATE as f64 / (static_stats.avg_micros() / 1_000_000.0);
    let runtime_throughput = SAMPLE_RATE as f64 / (runtime_stats.avg_micros() / 1_000_000.0);

    println!("\n🚀 Real-time Performance:");
    println!("  Static:  {:.0}x real-time ({:.0} samples/sec)",
             static_throughput / SAMPLE_RATE as f64, static_throughput);
    println!("  Runtime: {:.0}x real-time ({:.0} samples/sec)",
             runtime_throughput / SAMPLE_RATE as f64, runtime_throughput);

    println!("\n✨ Feature Parity Achieved!");
    println!("   - Both modes use GraphInterface");
    println!("   - Identical graph! macro syntax");
    println!("   - Seamless mode switching for performance tuning\n");
}
