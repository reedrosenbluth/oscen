//! Whole-app benchmarks: a MIDI-driven polyphonic FM synth shaped like the
//! real example apps (`examples/pivot`, `examples/fm-synth`,
//! `examples/electric-piano`). Where `graph_blocks.rs` isolates single
//! compiler features, these measure how they compose at application scale:
//! ~120 nodes (8 voices) / ~240 nodes (16 voices), nested voice graphs, MIDI
//! event routing through a voice allocator, wide ramped-parameter broadcast,
//! and a `Frame<2>` stereo output.
//!
//! Scenarios per graph:
//! - `idle`      — no notes ever played: the cost of the synth sitting in a
//!                 DAW doing nothing.
//! - `chord`     — 8 sustained notes: steady-state playing cost.
//! - `midi_stream` — continuous arpeggio via raw MIDI bytes each block:
//!                 parser + allocator + sub-block event splitting.
//! - `automation` — sustained chord while the host moves ramped parameters
//!                 every block: tick_ramps + setter cost at scale.
//!
//! Baseline workflow matches graph_blocks.rs:
//!
//! ```text
//! cargo bench -p oscen --bench synth_app -- --save-baseline pre
//! cargo bench -p oscen --bench synth_app -- --baseline pre
//! ```
#![feature(inherent_associated_types)]

use criterion::{black_box, criterion_group, criterion_main, Criterion, Throughput};
use oscen::graph::EventInstance;
use oscen::midi::raw_midi_event;

#[path = "support/poly_synth.rs"]
mod poly_synth;
use poly_synth::{PolySynth16, PolySynth8};

const BLOCK: usize = 512;
const CHORD: [u8; 8] = [48, 52, 55, 59, 62, 65, 69, 72];

fn note_on(note: u8, velocity: u8, frame_offset: u32) -> EventInstance {
    EventInstance {
        frame_offset,
        payload: raw_midi_event(&[0x90, note, velocity]),
    }
}

fn note_off(note: u8, frame_offset: u32) -> EventInstance {
    EventInstance {
        frame_offset,
        payload: raw_midi_event(&[0x80, note, 0]),
    }
}

/// Hold an 8-note chord and run enough blocks for every envelope to settle
/// into sustain, so the benched iterations measure steady state.
macro_rules! sustain_chord {
    ($g:expr) => {{
        for (k, &note) in CHORD.iter().enumerate() {
            $g.midi_in.try_push(note_on(note, 100, k as u32)).unwrap();
        }
        for _ in 0..100 {
            $g.process_block(BLOCK);
        }
    }};
}

fn bench_idle(c: &mut Criterion) {
    let mut group = c.benchmark_group("app/idle");
    group.throughput(Throughput::Elements(BLOCK as u64));

    group.bench_function("voices8", |b| {
        let mut g = PolySynth8::new();
        g.init(48_000.0);
        b.iter(|| {
            g.process_block(BLOCK);
            black_box(g.out_block[BLOCK - 1]);
        });
    });

    group.bench_function("voices16", |b| {
        let mut g = PolySynth16::new();
        g.init(48_000.0);
        b.iter(|| {
            g.process_block(BLOCK);
            black_box(g.out_block[BLOCK - 1]);
        });
    });

    group.finish();
}

fn bench_chord(c: &mut Criterion) {
    let mut group = c.benchmark_group("app/chord");
    group.throughput(Throughput::Elements(BLOCK as u64));

    group.bench_function("voices8", |b| {
        let mut g = PolySynth8::new();
        g.init(48_000.0);
        sustain_chord!(g);
        b.iter(|| {
            g.process_block(BLOCK);
            black_box(g.out_block[BLOCK - 1]);
        });
    });

    // Same 8-note chord on the 16-voice graph: the delta against voices8 is
    // the marginal cost of 8 extra *idle* voices inside a busy graph.
    group.bench_function("voices16", |b| {
        let mut g = PolySynth16::new();
        g.init(48_000.0);
        sustain_chord!(g);
        b.iter(|| {
            g.process_block(BLOCK);
            black_box(g.out_block[BLOCK - 1]);
        });
    });

    group.finish();
}

fn bench_midi_stream(c: &mut Criterion) {
    let mut group = c.benchmark_group("app/midi_stream");
    group.throughput(Throughput::Elements(BLOCK as u64));

    // A running arpeggio: every block turns 4 notes on and the previous 4
    // off, at spread frame offsets — continuous realistic MIDI traffic.
    group.bench_function("voices8", |b| {
        let mut g = PolySynth8::new();
        g.init(48_000.0);
        let mut step = 0u32;
        b.iter(|| {
            for k in 0..4u32 {
                let prev = 48 + ((step + k) % 24) as u8;
                let next = 48 + ((step + k + 4) % 24) as u8;
                g.midi_in.try_push(note_off(prev, k * 97)).unwrap();
                g.midi_in.try_push(note_on(next, 100, k * 97 + 48)).unwrap();
            }
            step = (step + 4) % 24;
            g.process_block(BLOCK);
            black_box(g.out_block[BLOCK - 1]);
        });
    });

    group.finish();
}

fn bench_automation(c: &mut Criterion) {
    let mut group = c.benchmark_group("app/automation");
    group.throughput(Throughput::Elements(BLOCK as u64));

    // Sustained chord while the host sweeps ramped parameters every block
    // (filter cutoff, FM routing, modulator level) — the "knob turn while
    // playing" workload.
    group.bench_function("voices8", |b| {
        let mut g = PolySynth8::new();
        g.init(48_000.0);
        sustain_chord!(g);
        let mut flip = false;
        b.iter(|| {
            flip = !flip;
            g.set_cutoff(if flip { 800.0 } else { 3200.0 });
            g.set_route(if flip { 0.2 } else { 0.8 });
            g.set_op3_level(if flip { 0.3 } else { 0.7 });
            g.process_block(BLOCK);
            black_box(g.out_block[BLOCK - 1]);
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_idle,
    bench_chord,
    bench_midi_stream,
    bench_automation
);
criterion_main!(benches);
