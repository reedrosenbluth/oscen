use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use oscen::{AdsrEnvelope, Delay, Graph, Oscillator, PolyBlepOscillator, TptFilter};
use oscen::graph::jit::{CraneliftJit, GraphStateBuilder};

fn simple_graph() -> Graph {
    let mut graph = Graph::new(44100.0);

    // Simple: 1 oscillator
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

fn bench_process_simple(c: &mut Criterion) {
    let mut group = c.benchmark_group("graph_process");

    group.bench_function("simple_graph", |b| {
        let mut graph = simple_graph();
        graph.validate().unwrap();

        b.iter(|| {
            black_box(graph.process().unwrap());
        });
    });

    group.finish();
}

fn bench_process_medium(c: &mut Criterion) {
    let mut group = c.benchmark_group("graph_process");

    group.bench_function("medium_graph", |b| {
        let mut graph = medium_graph();
        graph.validate().unwrap();

        b.iter(|| {
            black_box(graph.process().unwrap());
        });
    });

    group.finish();
}

fn bench_process_complex(c: &mut Criterion) {
    let mut group = c.benchmark_group("graph_process");

    group.bench_function("complex_graph", |b| {
        let mut graph = complex_graph();
        graph.validate().unwrap();

        b.iter(|| {
            black_box(graph.process().unwrap());
        });
    });

    group.finish();
}

fn bench_process_batch(c: &mut Criterion) {
    let mut group = c.benchmark_group("graph_process_batch");

    for size in [1, 10, 100, 512, 1024].iter() {
        group.bench_with_input(BenchmarkId::new("medium_graph", size), size, |b, &size| {
            let mut graph = medium_graph();
            graph.validate().unwrap();

            b.iter(|| {
                for _ in 0..size {
                    black_box(graph.process().unwrap());
                }
            });
        });
    }

    group.finish();
}

fn bench_topology_update(c: &mut Criterion) {
    let mut group = c.benchmark_group("topology");

    group.bench_function("topology_sort_medium", |b| {
        b.iter(|| {
            let mut graph = medium_graph();
            black_box(graph.validate().unwrap());
        });
    });

    group.bench_function("topology_sort_complex", |b| {
        b.iter(|| {
            let mut graph = complex_graph();
            black_box(graph.validate().unwrap());
        });
    });

    group.finish();
}

fn bench_jit_process_simple(c: &mut Criterion) {
    let mut group = c.benchmark_group("graph_process_jit");

    group.bench_function("simple_graph_jit", |b| {
        let mut graph = simple_graph();
        graph.validate().unwrap();

        let ir = graph.to_ir().expect("Failed to extract IR");
        let mut jit = CraneliftJit::new().expect("Failed to create JIT");
        let compiled = jit.compile(&ir).expect("Failed to compile");
        let mut state_builder = GraphStateBuilder::new(&ir, &mut graph.nodes);

        b.iter(|| {
            let (mut state, _temps) = state_builder.build(&mut graph.nodes, &mut graph.endpoints);
            black_box(compiled.process(&mut state));
        });
    });

    group.finish();
}

fn bench_jit_process_medium(c: &mut Criterion) {
    let mut group = c.benchmark_group("graph_process_jit");

    group.bench_function("medium_graph_jit", |b| {
        let mut graph = medium_graph();
        graph.validate().unwrap();

        let ir = graph.to_ir().expect("Failed to extract IR");
        let mut jit = CraneliftJit::new().expect("Failed to create JIT");
        let compiled = jit.compile(&ir).expect("Failed to compile");
        let mut state_builder = GraphStateBuilder::new(&ir, &mut graph.nodes);

        b.iter(|| {
            let (mut state, _temps) = state_builder.build(&mut graph.nodes, &mut graph.endpoints);
            black_box(compiled.process(&mut state));
        });
    });

    group.finish();
}

fn bench_jit_process_complex(c: &mut Criterion) {
    let mut group = c.benchmark_group("graph_process_jit");

    group.bench_function("complex_graph_jit", |b| {
        let mut graph = complex_graph();
        graph.validate().unwrap();

        let ir = graph.to_ir().expect("Failed to extract IR");
        let mut jit = CraneliftJit::new().expect("Failed to create JIT");
        let compiled = jit.compile(&ir).expect("Failed to compile");
        let mut state_builder = GraphStateBuilder::new(&ir, &mut graph.nodes);

        b.iter(|| {
            let (mut state, _temps) = state_builder.build(&mut graph.nodes, &mut graph.endpoints);
            black_box(compiled.process(&mut state));
        });
    });

    group.finish();
}

fn bench_jit_process_batch(c: &mut Criterion) {
    let mut group = c.benchmark_group("graph_process_batch_jit");

    for size in [1, 10, 100, 512, 1024].iter() {
        group.bench_with_input(BenchmarkId::new("medium_graph_jit", size), size, |b, &size| {
            let mut graph = medium_graph();
            graph.validate().unwrap();

            let ir = graph.to_ir().expect("Failed to extract IR");
            let mut jit = CraneliftJit::new().expect("Failed to create JIT");
            let compiled = jit.compile(&ir).expect("Failed to compile");
            let mut state_builder = GraphStateBuilder::new(&ir, &mut graph.nodes);

            b.iter(|| {
                for _ in 0..size {
                    let (mut state, _temps) = state_builder.build(&mut graph.nodes, &mut graph.endpoints);
                    black_box(compiled.process(&mut state));
                }
            });
        });
    }

    group.finish();
}

fn bench_jit_compilation(c: &mut Criterion) {
    let mut group = c.benchmark_group("jit_compilation");

    group.bench_function("compile_simple_graph", |b| {
        b.iter(|| {
            let mut graph = simple_graph();
            graph.validate().unwrap();

            let ir = graph.to_ir().expect("Failed to extract IR");
            let mut jit = CraneliftJit::new().expect("Failed to create JIT");
            black_box(jit.compile(&ir).expect("Failed to compile"));
        });
    });

    group.bench_function("compile_medium_graph", |b| {
        b.iter(|| {
            let mut graph = medium_graph();
            graph.validate().unwrap();

            let ir = graph.to_ir().expect("Failed to extract IR");
            let mut jit = CraneliftJit::new().expect("Failed to create JIT");
            black_box(jit.compile(&ir).expect("Failed to compile"));
        });
    });

    group.bench_function("compile_complex_graph", |b| {
        b.iter(|| {
            let mut graph = complex_graph();
            graph.validate().unwrap();

            let ir = graph.to_ir().expect("Failed to extract IR");
            let mut jit = CraneliftJit::new().expect("Failed to create JIT");
            black_box(jit.compile(&ir).expect("Failed to compile"));
        });
    });

    group.finish();
}

fn bench_interpreted_vs_jit(c: &mut Criterion) {
    let mut group = c.benchmark_group("interpreted_vs_jit");

    // Simple graph comparison
    group.bench_function("simple_interpreted", |b| {
        let mut graph = simple_graph();
        graph.validate().unwrap();

        b.iter(|| {
            black_box(graph.process().unwrap());
        });
    });

    group.bench_function("simple_jit", |b| {
        let mut graph = simple_graph();
        graph.validate().unwrap();

        let ir = graph.to_ir().expect("Failed to extract IR");
        let mut jit = CraneliftJit::new().expect("Failed to create JIT");
        let compiled = jit.compile(&ir).expect("Failed to compile");
        let mut state_builder = GraphStateBuilder::new(&ir, &mut graph.nodes);

        b.iter(|| {
            let (mut state, _temps) = state_builder.build(&mut graph.nodes, &mut graph.endpoints);
            black_box(compiled.process(&mut state));
        });
    });

    // Medium graph comparison
    group.bench_function("medium_interpreted", |b| {
        let mut graph = medium_graph();
        graph.validate().unwrap();

        b.iter(|| {
            black_box(graph.process().unwrap());
        });
    });

    group.bench_function("medium_jit", |b| {
        let mut graph = medium_graph();
        graph.validate().unwrap();

        let ir = graph.to_ir().expect("Failed to extract IR");
        let mut jit = CraneliftJit::new().expect("Failed to create JIT");
        let compiled = jit.compile(&ir).expect("Failed to compile");
        let mut state_builder = GraphStateBuilder::new(&ir, &mut graph.nodes);

        b.iter(|| {
            let (mut state, _temps) = state_builder.build(&mut graph.nodes, &mut graph.endpoints);
            black_box(compiled.process(&mut state));
        });
    });

    // Complex graph comparison
    group.bench_function("complex_interpreted", |b| {
        let mut graph = complex_graph();
        graph.validate().unwrap();

        b.iter(|| {
            black_box(graph.process().unwrap());
        });
    });

    group.bench_function("complex_jit", |b| {
        let mut graph = complex_graph();
        graph.validate().unwrap();

        let ir = graph.to_ir().expect("Failed to extract IR");
        let mut jit = CraneliftJit::new().expect("Failed to create JIT");
        let compiled = jit.compile(&ir).expect("Failed to compile");
        let mut state_builder = GraphStateBuilder::new(&ir, &mut graph.nodes);

        b.iter(|| {
            let (mut state, _temps) = state_builder.build(&mut graph.nodes, &mut graph.endpoints);
            black_box(compiled.process(&mut state));
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_process_simple,
    bench_process_medium,
    bench_process_complex,
    bench_process_batch,
    bench_topology_update,
    bench_jit_process_simple,
    bench_jit_process_medium,
    bench_jit_process_complex,
    bench_jit_process_batch,
    bench_jit_compilation,
    bench_interpreted_vs_jit
);
criterion_main!(benches);
