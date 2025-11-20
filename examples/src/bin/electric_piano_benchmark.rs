/// Benchmark comparing static vs runtime graph performance
/// Uses the GraphInterface trait to write mode-agnostic code

use oscen::prelude::*;
use oscen::graph::GraphInterface;
use std::time::{Duration, Instant};

// Include the electric piano voice modules
include!("../../electric-piano/src/electric_piano_voice.rs");
include!("../../electric-piano/src/tremolo.rs");

// Static version (compile_time: true)
graph! {
    name: ElectricPianoStatic;
    compile_time: true;

    input midi_in: event;
    input brightness: value = 30.0;
    input velocity_scaling: value = 50.0;
    input decay_rate: value = 90.0;
    input harmonic_decay: value = 70.0;
    input key_scaling: value = 50.0;
    input release_rate: value = 40.0;
    input vibrato_intensity: value = 0.3;
    input vibrato_speed: value = 5.0;

    output note_on_out: event;
    output note_off_out: event;
    output gate_witness: event;
    output left_out: stream;
    output right_out: stream;

    nodes {
        midi_parser = MidiParser::new();
        voice_allocator = VoiceAllocator::<16>::new(sample_rate);
        voice_handlers = [MidiVoiceHandler::new(); 16];
        voices = [ElectricPianoVoiceNode::new(sample_rate); 16];
        tremolo = Tremolo::new(sample_rate);
    }

    connections {
        midi_in -> midi_parser.midi_in;
        midi_parser.note_on -> note_on_out;
        midi_parser.note_off -> note_off_out;
        voice_handlers[0].gate -> gate_witness;

        midi_parser.note_on -> voice_allocator.note_on;
        midi_parser.note_off -> voice_allocator.note_off;

        voice_allocator.voices -> voice_handlers.note_on;
        voice_allocator.voices -> voice_handlers.note_off;

        voice_handlers.frequency -> voices.frequency;
        voice_handlers.gate -> voices.gate;

        brightness -> voices.brightness;
        velocity_scaling -> voices.velocity_scaling;
        decay_rate -> voices.decay_rate;
        harmonic_decay -> voices.harmonic_decay;
        key_scaling -> voices.key_scaling;
        release_rate -> voices.release_rate;

        voices.output -> tremolo.input;
        vibrato_intensity -> tremolo.depth;
        vibrato_speed -> tremolo.rate;

        tremolo.left_output -> left_out;
        tremolo.right_output -> right_out;
    }
}

// Runtime version (compile_time: false)
graph! {
    name: ElectricPianoRuntime;
    compile_time: false;

    input midi_in: event;
    input brightness: value = 30.0;
    input velocity_scaling: value = 50.0;
    input decay_rate: value = 90.0;
    input harmonic_decay: value = 70.0;
    input key_scaling: value = 50.0;
    input release_rate: value = 40.0;
    input vibrato_intensity: value = 0.3;
    input vibrato_speed: value = 5.0;

    output note_on_out: event;
    output note_off_out: event;
    output gate_witness: event;
    output left_out: stream;
    output right_out: stream;

    nodes {
        midi_parser = MidiParser::new();
        voice_allocator = VoiceAllocator::<16>::new(sample_rate);
        voice_handlers = [MidiVoiceHandler::new(); 16];
        voices = [ElectricPianoVoiceNode::new(sample_rate); 16];
        tremolo = Tremolo::new(sample_rate);
    }

    connections {
        midi_in -> midi_parser.midi_in;
        midi_parser.note_on -> note_on_out;
        midi_parser.note_off -> note_off_out;
        voice_handlers[0].gate -> gate_witness;

        midi_parser.note_on -> voice_allocator.note_on;
        midi_parser.note_off -> voice_allocator.note_off;

        voice_allocator.voices -> voice_handlers.note_on;
        voice_allocator.voices -> voice_handlers.note_off;

        voice_handlers.frequency -> voices.frequency;
        voice_handlers.gate -> voices.gate;

        brightness -> voices.brightness;
        velocity_scaling -> voices.velocity_scaling;
        decay_rate -> voices.decay_rate;
        harmonic_decay -> voices.harmonic_decay;
        key_scaling -> voices.key_scaling;
        release_rate -> voices.release_rate;

        voices.output -> tremolo.input;
        vibrato_intensity -> tremolo.depth;
        vibrato_speed -> tremolo.rate;

        tremolo.left_output -> left_out;
        tremolo.right_output -> right_out;
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
fn benchmark_graph<G: GraphInterface>(mut graph: G, name: &str, samples: usize) -> BenchmarkStats {
    let mut stats = BenchmarkStats::new();

    // Set some parameters using unified API
    graph.set_input_value("brightness", 50.0);
    graph.set_input_value("decay_rate", 80.0);

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

    println!("\n🎹 Electric Piano Benchmark");
    println!("============================\n");

    // Benchmark static graph
    let static_graph = ElectricPianoStatic::new(SAMPLE_RATE);
    let static_stats = benchmark_graph(static_graph, "Static Graph (compile_time: true)", BENCHMARK_SAMPLES);

    // Benchmark runtime graph
    let runtime_graph = ElectricPianoRuntime::new(SAMPLE_RATE);
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
    let static_throughput = SAMPLE_RATE / (static_stats.avg_micros() / 1_000_000.0);
    let runtime_throughput = SAMPLE_RATE / (runtime_stats.avg_micros() / 1_000_000.0);

    println!("\n🚀 Real-time Performance:");
    println!("  Static:  {:.0}x real-time ({:.0} samples/sec)",
             static_throughput / SAMPLE_RATE, static_throughput);
    println!("  Runtime: {:.0}x real-time ({:.0} samples/sec)",
             runtime_throughput / SAMPLE_RATE, runtime_throughput);

    println!("\n✨ Both modes implement GraphInterface!");
    println!("   Code can switch between modes seamlessly.\n");
}
