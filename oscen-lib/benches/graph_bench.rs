use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkId};
use oscen::{Graph, Oscillator, PolyBlepOscillator, TptFilter, AdsrEnvelope, Delay};

fn simple_graph() -> Graph {
    let mut graph = Graph::new(44100.0);

    // Simple: 1 oscillator
    let osc = graph.add_node(Oscillator::sine(440.0, 1.0));
    graph
}

fn medium_graph() -> Graph {
    let mut graph = Graph::new(44100.0);

    // Medium: 2 oscillators + filter + envelope
    let osc1 = graph.add_node(Oscillator::sine(440.0, 1.0));
    let osc2 = graph.add_node(PolyBlepOscillator::saw(442.0, 1.0));
    let filter = graph.add_node(TptFilter::new(1000.0, 0.7));
    let env = graph.add_node(AdsrEnvelope::new(0.01, 0.1, 0.7, 0.2));

    let mixed = graph.add(osc1.output(), osc2.output());
    graph.connect(mixed, filter.input());

    let _final_out = graph.multiply(filter.output(), env.output());

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
    let mix1 = graph.add(osc1.output(), osc2.output());
    let mix2 = graph.add(mix1, osc3.output());

    // Mix last 2 oscillators
    let mix3 = graph.add(osc4.output(), osc5.output());

    // Filter each mix
    let filter1 = graph.add_node(TptFilter::new(800.0, 0.5));
    let filter2 = graph.add_node(TptFilter::new(1200.0, 0.5));

    graph.connect(mix2, filter1.input());
    graph.connect(mix3, filter2.input());

    // Envelopes
    let env1 = graph.add_node(AdsrEnvelope::new(0.01, 0.1, 0.7, 0.2));
    let env2 = graph.add_node(AdsrEnvelope::new(0.02, 0.15, 0.6, 0.3));

    // Apply envelopes
    let filtered1 = graph.multiply(filter1.output(), env1.output());
    let filtered2 = graph.multiply(filter2.output(), env2.output());

    // Mix and delay
    let final_mix = graph.add(filtered1, filtered2);
    let delay = graph.add_node(Delay::new(0.5, 0.3));

    graph.connect(final_mix, delay.input());

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

criterion_group!(
    benches,
    bench_process_simple,
    bench_process_medium,
    bench_process_complex,
    bench_process_batch,
    bench_topology_update
);
criterion_main!(benches);
