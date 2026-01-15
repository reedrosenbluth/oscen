use criterion::{black_box, criterion_group, criterion_main, Criterion};
use oscen::{graph, AdsrEnvelope, Gain, Oscillator, PolyBlepOscillator, SignalProcessor, TptFilter};

// Simple graph for baseline comparison
graph! {
    name: StaticSimpleGraph;

    nodes {
        osc = Oscillator::sine(440.0, 1.0);
        filter = TptFilter::new(1000.0, 0.7);
        gain = Gain::new(0.5);
    }

    connections {
        osc.output -> filter.input;
        filter.output -> gain.input;
    }
}

// Complex synthesizer voice with multiple oscillators, envelopes, and mixing
graph! {
    name: StaticComplexGraph;

    // Three detuned oscillators for unison effect
    nodes {
        osc1 = PolyBlepOscillator::saw(440.0, 0.33);
        osc2 = PolyBlepOscillator::saw(442.0, 0.33);  // +2 Hz detune
        osc3 = PolyBlepOscillator::saw(438.0, 0.33);  // -2 Hz detune

        // Mix the three oscillators
        mix1 = Gain::new(1.0);
        mix2 = Gain::new(1.0);
        mix3 = Gain::new(1.0);
        mixer = Gain::new(1.0);

        // Filter envelope for cutoff modulation
        filter_env = AdsrEnvelope::new(0.01, 0.3, 0.5, 0.2);
        env_amount = Gain::new(2000.0);  // Envelope modulation amount

        // Main filter
        filter = TptFilter::new(800.0, 0.7);

        // Amplitude envelope
        amp_env = AdsrEnvelope::new(0.01, 0.2, 0.7, 0.3);
        vca = Gain::new(1.0);
    }

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

fn bench_static_simple(c: &mut Criterion) {
    let mut group = c.benchmark_group("simple_graph");

    // Benchmark compile-time static graph (simple)
    group.bench_function("static", |b| {
        let mut graph = StaticSimpleGraph::new();
        graph.init(44100.0);

        b.iter(|| {
            black_box(graph.process());
        });
    });

    group.finish();
}

fn bench_static_complex(c: &mut Criterion) {
    let mut group = c.benchmark_group("complex_graph");

    // Benchmark compile-time static graph (complex)
    group.bench_function("static", |b| {
        let mut graph = StaticComplexGraph::new();
        graph.init(44100.0);

        b.iter(|| {
            black_box(graph.process());
        });
    });

    group.finish();
}

fn bench_batch_processing(c: &mut Criterion) {
    let mut group = c.benchmark_group("batch_processing");

    // Process 512 samples (typical audio buffer size)
    group.bench_function("static_graph_512", |b| {
        let mut graph = StaticSimpleGraph::new();
        graph.init(44100.0);

        b.iter(|| {
            for _ in 0..512 {
                black_box(graph.process());
            }
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_static_simple,
    bench_static_complex,
    bench_batch_processing
);
criterion_main!(benches);
