use criterion::{black_box, criterion_group, criterion_main, Criterion};
use oscen::{graph, Graph, PolyBlepOscillator, TptFilter};

// Define compile-time static graph
graph! {
    name: StaticSimpleGraph;
    compile_time: true;

    node osc = PolyBlepOscillator::saw(440.0, 1.0);
    node filter = TptFilter::new(1000.0, 0.7);

    connections {
        osc.output -> filter.input;
    }
}

// Runtime graph with same structure
fn runtime_simple_graph() -> Graph {
    let mut graph = Graph::new(44100.0);

    let osc = graph.add_node(PolyBlepOscillator::saw(440.0, 1.0));
    let filter = graph.add_node(TptFilter::new(1000.0, 0.7));

    graph.connect(osc.output, filter.input);
    graph.validate().unwrap();

    graph
}

fn bench_static_vs_runtime(c: &mut Criterion) {
    let mut group = c.benchmark_group("static_vs_runtime");

    // Benchmark compile-time static graph
    group.bench_function("static_graph", |b| {
        let mut graph = StaticSimpleGraph::new(44100.0);

        b.iter(|| {
            black_box(graph.process());
        });
    });

    // Benchmark runtime dynamic graph
    group.bench_function("runtime_graph", |b| {
        let mut graph = runtime_simple_graph();

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

criterion_group!(benches, bench_static_vs_runtime, bench_batch_processing);
criterion_main!(benches);
