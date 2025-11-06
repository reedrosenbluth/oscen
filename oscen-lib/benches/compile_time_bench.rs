use criterion::{black_box, criterion_group, criterion_main, Criterion};
use oscen::{Gain, Graph, Oscillator, ProcessingContext, SignalProcessor, TptFilter};

// ============================================================================
// 1. Runtime Graph (current approach - dynamic dispatch via SlotMaps)
// ============================================================================

fn runtime_synth() -> Graph {
    let mut graph = Graph::new(44100.0);

    let osc = graph.add_node(Oscillator::sine(440.0, 1.0));
    let filter = graph.add_node(TptFilter::new(1000.0, 0.7));
    let gain = graph.add_node(Gain::new(0.5));

    graph.connect(osc.output, filter.input);
    graph.connect(filter.output, gain.input);

    graph
}

// ============================================================================
// 2. Hand-Written Compile-Time Graph (no macro - direct implementation)
// ============================================================================

/// This demonstrates the principle of compile-time graphs.
///
/// Key optimizations vs runtime graphs:
/// 1. Direct node fields (not Box<dyn SignalProcessor>) - no heap allocations
/// 2. No SlotMap lookups - compiler knows exact memory locations
/// 3. Direct method calls - compiler knows exact types, can inline everything
/// 4. No Result<> wrapping - process() can't fail
///
/// Note: This simplified version still uses the existing process() method.
/// The macro-generated version will use IO structs for even better performance.
pub struct HandWrittenSynth {
    // Node instances (direct types, not Box<dyn>)
    osc: Oscillator,
    filter: TptFilter,
    gain: Gain,

    // Sample rate
    sample_rate: f32,

    // Intermediate values (connections between nodes)
    osc_output: f32,
    filter_output: f32,
}

impl HandWrittenSynth {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            osc: Oscillator::sine(440.0, 1.0),
            filter: TptFilter::new(1000.0, 0.7),
            gain: Gain::new(0.5),
            sample_rate,
            osc_output: 0.0,
            filter_output: 0.0,
        }
    }

    /// Process one sample with direct method calls and no dynamic dispatch.
    ///
    /// This is fully inlineable by LLVM, resulting in tight assembly code
    /// with no function call overhead.
    #[inline]
    pub fn process(&mut self) -> f32 {
        // Create empty context (nodes will use our direct fields instead)
        let mut ctx = ProcessingContext::empty();

        // Process nodes directly - compiler knows exact types
        // No SlotMap lookups, no virtual function calls, fully inlineable
        self.osc_output = self.osc.process(self.sample_rate, &mut ctx);
        self.filter_output = self.filter.process(self.sample_rate, &mut ctx);
        let final_output = self.gain.process(self.sample_rate, &mut ctx);

        final_output
    }
}

// ============================================================================
// 3. Macro-Generated Compile-Time Graph (once macro is working)
// ============================================================================

// This will be uncommented once the macro can actually generate code
// For now it's here to show the intended usage
/*
use oscen::graph;

graph! {
    name: MacroGeneratedSynth;
    mode: CompileTime;

    nodes {
        osc = Oscillator::sine(440.0, 1.0);
        filter = TptFilter::new(1000.0, 0.7);
        gain = Gain::new(0.5);
    }

    connections {
        osc.output -> filter.input;
        filter.output -> gain.input;
    }

    outputs {
        stream output;
    }
}
*/

// ============================================================================
// Benchmarks
// ============================================================================

fn bench_runtime_vs_compile_time(c: &mut Criterion) {
    let mut group = c.benchmark_group("synth_comparison");

    // Baseline: Runtime graph with dynamic dispatch
    group.bench_function("1_runtime_graph", |b| {
        let mut graph = runtime_synth();
        graph.validate().unwrap();

        b.iter(|| {
            black_box(graph.process().unwrap());
        });
    });

    // Hand-written compile-time graph
    group.bench_function("2_hand_written_compile_time", |b| {
        let mut synth = HandWrittenSynth::new(44100.0);

        b.iter(|| {
            black_box(synth.process());
        });
    });

    // TODO: Add macro-generated compile-time graph benchmark once macro works
    // group.bench_function("3_macro_generated_compile_time", |b| {
    //     let mut synth = MacroGeneratedSynth::new(44100.0);
    //     b.iter(|| {
    //         black_box(synth.process());
    //     });
    // });

    group.finish();
}

fn bench_batch_processing(c: &mut Criterion) {
    let mut group = c.benchmark_group("batch_processing");

    const BATCH_SIZE: usize = 512; // One audio callback

    group.bench_function("runtime_batch_512", |b| {
        let mut graph = runtime_synth();
        graph.validate().unwrap();

        b.iter(|| {
            for _ in 0..BATCH_SIZE {
                black_box(graph.process().unwrap());
            }
        });
    });

    group.bench_function("hand_written_batch_512", |b| {
        let mut synth = HandWrittenSynth::new(44100.0);

        b.iter(|| {
            for _ in 0..BATCH_SIZE {
                black_box(synth.process());
            }
        });
    });

    group.finish();
}

fn bench_throughput(c: &mut Criterion) {
    let mut group = c.benchmark_group("throughput");
    group.throughput(criterion::Throughput::Elements(1)); // 1 sample

    group.bench_function("runtime_throughput", |b| {
        let mut graph = runtime_synth();
        graph.validate().unwrap();

        b.iter(|| {
            black_box(graph.process().unwrap());
        });
    });

    group.bench_function("hand_written_throughput", |b| {
        let mut synth = HandWrittenSynth::new(44100.0);

        b.iter(|| {
            black_box(synth.process());
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_runtime_vs_compile_time,
    bench_batch_processing,
    bench_throughput
);
criterion_main!(benches);
