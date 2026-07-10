//! Block-path benchmarks for generated graphs.
//!
//! Every group reports throughput in frames (`Throughput::Elements`), so
//! criterion prints a comparable ns/frame figure across block sizes and graph
//! shapes. Baseline workflow:
//!
//! ```text
//! cargo bench -p oscen --bench graph_blocks -- --save-baseline pre
//! # ...make a change...
//! cargo bench -p oscen --bench graph_blocks -- --baseline pre
//! ```
#![allow(non_camel_case_types)]

use criterion::{
    black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput,
};
use oscen::graph::{EventInstance, EventPayload};
use oscen::{graph, oversample_variants, AdsrEnvelope, Gain, PolyBlepOscillator, TptFilter};

const BLOCK_SIZES: [usize; 3] = [64, 128, 512];

// ---------------------------------------------------------------------------
// Passthrough: 1 stream in, 1 stream out, trivial node work. Isolates the
// per-frame framework overhead (block<->scalar copies, connect chain).
// ---------------------------------------------------------------------------

graph! {
    name: PassthroughGraph;

    input stream audio_in;
    output stream audio_out;

    nodes {
        gain = Gain::new(0.5);
    }

    connections {
        audio_in -> gain.input;
        gain.output -> audio_out;
    }
}

fn bench_passthrough(c: &mut Criterion) {
    let mut group = c.benchmark_group("block/passthrough");
    for &n in &BLOCK_SIZES {
        group.throughput(Throughput::Elements(n as u64));
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, &n| {
            let mut g = PassthroughGraph::new();
            g.init(48_000.0);
            for i in 0..n {
                g.audio_in_block[i] = (i as f32 * 0.01).sin();
            }
            b.iter(|| {
                g.process_block(n);
                black_box(g.audio_out_block[n - 1]);
            });
        });
    }
    group.finish();

    // Same graph driven per-sample: the delta against process_block(512) IS
    // the block-path overhead.
    let mut group = c.benchmark_group("block/passthrough_persample");
    group.throughput(Throughput::Elements(512));
    group.bench_function("512", |b| {
        let mut g = PassthroughGraph::new();
        g.init(48_000.0);
        b.iter(|| {
            for i in 0..512 {
                g.audio_in = (i as f32 * 0.01).sin();
                g.process();
                black_box(g.audio_out);
            }
        });
    });
    group.finish();
}

// ---------------------------------------------------------------------------
// Voice: FM-synth-voice shaped graph — 3 detuned oscillators summed via a
// compound source, filter + envelope modulation, VCA, an event gate, and a
// ramped value input (exercises tick_ramps).
// ---------------------------------------------------------------------------

graph! {
    name: VoiceGraph;

    input gate: event;
    input cutoff: value = 1200.0 [20.0..20000.0, ramp: 64];
    output audio_out: stream;

    nodes {
        osc1 = PolyBlepOscillator::saw(440.0, 0.33);
        osc2 = PolyBlepOscillator::saw(442.0, 0.33);
        osc3 = PolyBlepOscillator::saw(438.0, 0.33);
        filter_env = AdsrEnvelope::new(0.01, 0.3, 0.5, 0.2);
        env_amount = Gain::new(2000.0);
        filter = TptFilter::new(800.0, 0.7);
        amp_env = AdsrEnvelope::new(0.01, 0.2, 0.7, 0.3);
        vca = Gain::new(1.0);
    }

    connections {
        gate -> filter_env.gate, amp_env.gate;
        cutoff -> filter.cutoff;

        osc1.output + osc2.output + osc3.output -> filter.input;
        filter_env.output -> env_amount.input;
        env_amount.output -> filter.f_mod;
        filter.output -> vca.input;
        amp_env.output -> vca.gain;
        vca.output -> audio_out;
    }
}

fn gate_on(frame_offset: u32) -> EventInstance {
    EventInstance {
        frame_offset,
        payload: EventPayload::scalar(1.0),
    }
}

fn bench_voice(c: &mut Criterion) {
    let mut group = c.benchmark_group("block/voice_silent");
    for &n in &BLOCK_SIZES {
        group.throughput(Throughput::Elements(n as u64));
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, &n| {
            let mut g = VoiceGraph::new();
            g.init(48_000.0);
            b.iter(|| {
                g.process_block(n);
                black_box(g.audio_out_block[n - 1]);
            });
        });
    }
    group.finish();

    let mut group = c.benchmark_group("block/voice_gated");
    for &n in &BLOCK_SIZES {
        group.throughput(Throughput::Elements(n as u64));
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, &n| {
            let mut g = VoiceGraph::new();
            g.init(48_000.0);
            let mut cutoff = 800.0f32;
            b.iter(|| {
                // A gate at frame 0 plus a ramped param move every block.
                g.gate.try_push(gate_on(0)).unwrap();
                cutoff = if cutoff > 1000.0 { 800.0 } else { 1600.0 };
                g.set_cutoff(cutoff);
                g.process_block(n);
                black_box(g.audio_out_block[n - 1]);
            });
        });
    }
    group.finish();
}

// ---------------------------------------------------------------------------
// Events: event input broadcast into an 8-wide node array, plus a second
// event input into a scalar node. Measures the sub-block splitting and event
// staging/clone costs of process_block.
// ---------------------------------------------------------------------------

graph! {
    name: EventArrayGraph;

    input gate_a: event;
    input gate_b: event;
    output audio_out: stream;

    nodes {
        envs = [AdsrEnvelope::new(0.001, 0.05, 0.5, 0.1); 8];
        solo = AdsrEnvelope::new(0.001, 0.05, 0.5, 0.1);
        mix = Gain::new(0.125);
    }

    connections {
        gate_a -> envs.gate;
        gate_b -> solo.gate;

        envs.output -> mix.input;
        solo.output -> mix.gain;
        mix.output -> audio_out;
    }
}

fn bench_events(c: &mut Criterion) {
    let mut group = c.benchmark_group("block/events");
    group.throughput(Throughput::Elements(512));

    // No events queued: pure per-frame event-plumbing overhead.
    group.bench_function("silent512", |b| {
        let mut g = EventArrayGraph::new();
        g.init(48_000.0);
        b.iter(|| {
            g.process_block(512);
            black_box(g.audio_out_block[511]);
        });
    });

    // 16 events at spread offsets: staging + sort + sub-block boundaries.
    group.bench_function("burst16", |b| {
        let mut g = EventArrayGraph::new();
        g.init(48_000.0);
        b.iter(|| {
            for k in 0..16u32 {
                g.gate_a.try_push(gate_on(k * 31)).unwrap();
            }
            g.process_block(512);
            black_box(g.audio_out_block[511]);
        });
    });

    // Full 128-event queue at frame 0 (MIDI-panic shape): worst-case clones.
    group.bench_function("midiflood", |b| {
        let mut g = EventArrayGraph::new();
        g.init(48_000.0);
        b.iter(|| {
            for _ in 0..128 {
                g.gate_a.try_push(gate_on(0)).unwrap();
            }
            g.process_block(512);
            black_box(g.audio_out_block[511]);
        });
    });

    group.finish();
}

// ---------------------------------------------------------------------------
// Multirate: same body at 1x (control) and 4x oversampling. The 4x/1x ratio
// tracks cross-rate kernel + inner-loop overhead.
// ---------------------------------------------------------------------------

oversample_variants! {
    base_name: OversampledOsc;
    factors: [1, 4];
    body: {
        output stream audio_out;
        nodes {
            osc = PolyBlepOscillator::saw(220.0, 0.8) * {FACTOR};
        }
        connections {
            [sinc] osc.output -> audio_out;
        }
    }
}

fn bench_multirate(c: &mut Criterion) {
    let mut group = c.benchmark_group("block/multirate");
    group.throughput(Throughput::Elements(512));

    group.bench_function("1x", |b| {
        let mut g = OversampledOsc_1x::new();
        g.init(48_000.0);
        b.iter(|| {
            g.process_block(512);
            black_box(g.audio_out_block[511]);
        });
    });

    group.bench_function("4x", |b| {
        let mut g = OversampledOsc_4x::new();
        g.init(48_000.0);
        b.iter(|| {
            g.process_block(512);
            black_box(g.audio_out_block[511]);
        });
    });

    group.finish();
}

// ---------------------------------------------------------------------------
// Array fan-in: [osc; 8] summed into a scalar stream input.
// ---------------------------------------------------------------------------

graph! {
    name: ArrayFaninGraph;

    output audio_out: stream;

    nodes {
        oscs = [PolyBlepOscillator::saw(220.0, 0.125); 8];
        filter = TptFilter::new(1000.0, 0.7);
    }

    connections {
        oscs.output -> filter.input;
        filter.output -> audio_out;
    }
}

fn bench_array_fanin(c: &mut Criterion) {
    let mut group = c.benchmark_group("block/array_fanin");
    group.throughput(Throughput::Elements(512));
    group.bench_function("8", |b| {
        let mut g = ArrayFaninGraph::new();
        g.init(48_000.0);
        b.iter(|| {
            g.process_block(512);
            black_box(g.audio_out_block[511]);
        });
    });
    group.finish();
}

criterion_group!(
    benches,
    bench_passthrough,
    bench_voice,
    bench_events,
    bench_multirate,
    bench_array_fanin
);
criterion_main!(benches);
