use criterion::{black_box, criterion_group, criterion_main, Criterion};
use oscen::{graph, AdsrEnvelope, Gain, Graph, Oscillator, PolyBlepOscillator, TptFilter};

// Simple graph for baseline comparison (matches old compile_time_bench)
graph! {
    name: StaticSimpleGraph;
    compile_time: true;

    node osc = Oscillator::sine(440.0, 1.0);
    node filter = TptFilter::new(1000.0, 0.7);
    node gain = Gain::new(0.5);

    connections {
        osc.output -> filter.input;
        filter.output -> gain.input;
    }
}

// Complex synthesizer voice with multiple oscillators, envelopes, and mixing
graph! {
    name: StaticComplexGraph;
    compile_time: true;

    // Three detuned oscillators for unison effect
    node osc1 = PolyBlepOscillator::saw(440.0, 0.33);
    node osc2 = PolyBlepOscillator::saw(442.0, 0.33);  // +2 Hz detune
    node osc3 = PolyBlepOscillator::saw(438.0, 0.33);  // -2 Hz detune

    // Mix the three oscillators
    node mix1 = Gain::new(1.0);
    node mix2 = Gain::new(1.0);
    node mix3 = Gain::new(1.0);
    node mixer = Gain::new(1.0);

    // Filter envelope for cutoff modulation
    node filter_env = AdsrEnvelope::new(0.01, 0.3, 0.5, 0.2);
    node env_amount = Gain::new(2000.0);  // Envelope modulation amount

    // Main filter
    node filter = TptFilter::new(800.0, 0.7);

    // Amplitude envelope
    node amp_env = AdsrEnvelope::new(0.01, 0.2, 0.7, 0.3);
    node vca = Gain::new(1.0);

    connections {
        // Mix oscillators
        osc1.output -> mix1.input;
        osc2.output -> mix2.input;
        osc3.output -> mix3.input;

        // Combine mixed signals (simple addition through gain stages)
        mix1.output -> mixer.input;

        // Filter with envelope modulation
        mixer.output -> filter.input;
        filter_env.output -> env_amount.input;
        env_amount.output -> filter.f_mod;

        // Amplitude envelope
        filter.output -> vca.input;
        amp_env.output -> vca.gain;
    }
}

// Runtime graph with simple structure (matches old compile_time_bench)
fn runtime_simple_graph() -> Graph {
    let mut graph = Graph::new(44100.0);

    let osc = graph.add_node(Oscillator::sine(440.0, 1.0));
    let filter = graph.add_node(TptFilter::new(1000.0, 0.7));
    let gain = graph.add_node(Gain::new(0.5));

    graph.connect(osc.output, filter.input);
    graph.connect(filter.output, gain.input);
    graph.validate().unwrap();

    graph
}

// Runtime graph with complex structure
fn runtime_complex_graph() -> Graph {
    let mut graph = Graph::new(44100.0);

    // Three detuned oscillators
    let osc1 = graph.add_node(PolyBlepOscillator::saw(440.0, 0.33));
    let osc2 = graph.add_node(PolyBlepOscillator::saw(442.0, 0.33));
    let osc3 = graph.add_node(PolyBlepOscillator::saw(438.0, 0.33));

    // Mix the oscillators
    let mix1 = graph.add_node(Gain::new(1.0));
    let mix2 = graph.add_node(Gain::new(1.0));
    let mix3 = graph.add_node(Gain::new(1.0));
    let mixer = graph.add_node(Gain::new(1.0));

    // Filter envelope
    let filter_env = graph.add_node(AdsrEnvelope::new(0.01, 0.3, 0.5, 0.2));
    let env_amount = graph.add_node(Gain::new(2000.0));

    // Main filter
    let filter = graph.add_node(TptFilter::new(800.0, 0.7));

    // Amplitude envelope
    let amp_env = graph.add_node(AdsrEnvelope::new(0.01, 0.2, 0.7, 0.3));
    let vca = graph.add_node(Gain::new(1.0));

    // Connect everything
    graph.connect(osc1.output, mix1.input);
    graph.connect(osc2.output, mix2.input);
    graph.connect(osc3.output, mix3.input);

    graph.connect(mix1.output, mixer.input);

    graph.connect(mixer.output, filter.input);
    graph.connect(filter_env.output, env_amount.input);
    graph.connect(env_amount.output, filter.f_mod);

    graph.connect(filter.output, vca.input);
    graph.connect(amp_env.output, vca.gain);

    graph.validate().unwrap();

    graph
}

fn bench_static_vs_runtime(c: &mut Criterion) {
    let mut group = c.benchmark_group("simple_graph");

    // Benchmark compile-time static graph (simple)
    group.bench_function("static", |b| {
        let mut graph = StaticSimpleGraph::new(44100.0);

        b.iter(|| {
            black_box(graph.process());
        });
    });

    // Benchmark runtime dynamic graph (simple)
    group.bench_function("runtime", |b| {
        let mut graph = runtime_simple_graph();

        b.iter(|| {
            black_box(graph.process().unwrap());
        });
    });

    group.finish();
}

fn bench_complex_graph(c: &mut Criterion) {
    let mut group = c.benchmark_group("complex_graph");

    // Benchmark compile-time static graph (complex)
    group.bench_function("static", |b| {
        let mut graph = StaticComplexGraph::new(44100.0);

        b.iter(|| {
            black_box(graph.process());
        });
    });

    // Benchmark runtime dynamic graph (complex)
    group.bench_function("runtime", |b| {
        let mut graph = runtime_complex_graph();

        b.iter(|| {
            black_box(graph.process().unwrap());
        });
    });

    group.finish();
}

fn bench_batch_processing(c: &mut Criterion) {
    let mut group = c.benchmark_group("batch_processing");

    // Process 512 samples (typical audio buffer size)
    group.bench_function("static_graph_512", |b| {
        let mut graph = StaticSimpleGraph::new(44100.0);

        b.iter(|| {
            for _ in 0..512 {
                black_box(graph.process());
            }
        });
    });

    group.bench_function("runtime_graph_512", |b| {
        let mut graph = runtime_simple_graph();

        b.iter(|| {
            for _ in 0..512 {
                black_box(graph.process().unwrap());
            }
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_static_vs_runtime,
    bench_complex_graph,
    bench_batch_processing
);
criterion_main!(benches);
