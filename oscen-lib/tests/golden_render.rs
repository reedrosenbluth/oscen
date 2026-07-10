//! Golden-output regression tests for generated graphs.
//!
//! Each test renders a deterministic input through a graph shape mirrored
//! from `benches/graph_blocks.rs` and compares an FNV-1a hash of the exact
//! output bit patterns (`f32::to_bits`) against a recorded constant. Any
//! codegen change that alters audio output — even by one ULP — fails here,
//! which per-sample-vs-block equivalence tests alone cannot catch (a change
//! that alters both paths identically slips through those).
//!
//! To re-record after an *intentional* audio change, run with
//! `OSCEN_PRINT_GOLDEN=1 cargo test -p oscen --test golden_render -- --nocapture`
//! and update the constants, justifying the diff in the commit message.

use oscen::graph::{EventInstance, EventPayload};
use oscen::{graph, oversample_variants, AdsrEnvelope, Gain, PolyBlepOscillator, TptFilter};

const RENDER_FRAMES: usize = 2048;
const BLOCK: usize = 512;

fn fnv1a64(bits: impl Iterator<Item = u32>) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in bits {
        for byte in b.to_le_bytes() {
            h ^= byte as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
    }
    h
}

/// Deterministic full-scale noise in [-1, 1).
struct Lcg(u64);
impl Lcg {
    fn next_f32(&mut self) -> f32 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((self.0 >> 40) as f32 / (1u32 << 24) as f32) * 2.0 - 1.0
    }
}

fn check(name: &str, expected: u64, actual: u64) {
    if std::env::var_os("OSCEN_PRINT_GOLDEN").is_some() {
        println!("{name}: 0x{actual:016x}");
        return;
    }
    assert_eq!(
        actual, expected,
        "{name}: golden output changed (got 0x{actual:016x}, expected 0x{expected:016x}); \
         if intentional, re-record with OSCEN_PRINT_GOLDEN=1"
    );
}

// --- Passthrough -----------------------------------------------------------

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

#[test]
fn golden_passthrough() {
    let mut g = PassthroughGraph::new();
    g.init(48_000.0);
    let mut lcg = Lcg(1);
    let mut out = Vec::with_capacity(RENDER_FRAMES);
    for _ in 0..RENDER_FRAMES / BLOCK {
        for i in 0..BLOCK {
            g.audio_in_block[i] = lcg.next_f32();
        }
        g.process_block(BLOCK);
        out.extend_from_slice(&g.audio_out_block[..BLOCK]);
    }
    check(
        "passthrough",
        0x6c6ddc5f1c7a30e4,
        fnv1a64(out.iter().map(|v| v.to_bits())),
    );
}

// --- Voice -----------------------------------------------------------------

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

#[test]
fn golden_voice() {
    let mut g = VoiceGraph::new();
    g.init(48_000.0);
    let mut out = Vec::with_capacity(RENDER_FRAMES);
    for block in 0..RENDER_FRAMES / BLOCK {
        if block == 0 {
            g.gate
                .try_push(EventInstance {
                    frame_offset: 0,
                    payload: EventPayload::scalar(1.0),
                })
                .unwrap();
        }
        if block == 1 {
            g.set_cutoff(1600.0); // exercise the ramp mid-render
        }
        g.process_block(BLOCK);
        out.extend_from_slice(&g.audio_out_block[..BLOCK]);
    }
    check("voice", 0xedc44a4a05d69c57, fnv1a64(out.iter().map(|v| v.to_bits())));
}

// --- Events into node array -------------------------------------------------

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

#[test]
fn golden_event_array() {
    let mut g = EventArrayGraph::new();
    g.init(48_000.0);
    let mut out = Vec::with_capacity(RENDER_FRAMES);
    for block in 0..RENDER_FRAMES / BLOCK {
        // Spread gates across the block; alternate velocities per block.
        for k in 0..16u32 {
            g.gate_a
                .try_push(EventInstance {
                    frame_offset: k * 31,
                    payload: EventPayload::scalar(if block % 2 == 0 { 1.0 } else { 0.5 }),
                })
                .unwrap();
        }
        g.gate_b
            .try_push(EventInstance {
                frame_offset: 100,
                payload: EventPayload::scalar(1.0),
            })
            .unwrap();
        g.process_block(BLOCK);
        out.extend_from_slice(&g.audio_out_block[..BLOCK]);
    }
    check("event_array", 0xd2942a2cffa882c6, fnv1a64(out.iter().map(|v| v.to_bits())));
}

// --- Multirate ---------------------------------------------------------------

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

#[test]
fn golden_multirate() {
    #![allow(non_camel_case_types)]
    let mut g1 = OversampledOsc_1x::new();
    let mut g4 = OversampledOsc_4x::new();
    g1.init(48_000.0);
    g4.init(48_000.0);
    let mut out1 = Vec::with_capacity(RENDER_FRAMES);
    let mut out4 = Vec::with_capacity(RENDER_FRAMES);
    for _ in 0..RENDER_FRAMES / BLOCK {
        g1.process_block(BLOCK);
        out1.extend_from_slice(&g1.audio_out_block[..BLOCK]);
        g4.process_block(BLOCK);
        out4.extend_from_slice(&g4.audio_out_block[..BLOCK]);
    }
    check("multirate_1x", 0x34b06c4b4a5e59e6, fnv1a64(out1.iter().map(|v| v.to_bits())));
    check("multirate_4x", 0x484510f55872ab97, fnv1a64(out4.iter().map(|v| v.to_bits())));
}

// --- Array fan-in -------------------------------------------------------------

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

#[test]
fn golden_array_fanin() {
    let mut g = ArrayFaninGraph::new();
    g.init(48_000.0);
    let mut out = Vec::with_capacity(RENDER_FRAMES);
    for _ in 0..RENDER_FRAMES / BLOCK {
        g.process_block(BLOCK);
        out.extend_from_slice(&g.audio_out_block[..BLOCK]);
    }
    check("array_fanin", 0x6fbc3d3719ff669f, fnv1a64(out.iter().map(|v| v.to_bits())));
}
