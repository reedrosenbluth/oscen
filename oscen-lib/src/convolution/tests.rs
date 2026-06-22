//! Tests for the buildable `ConvolverEngine`, the live IR swap, and the
//! equal-power crossfade. The IR-swap path is exercised by hand-building a
//! `handoff::pair` and an `AssetLoadHandle`; the `graph!` wiring is a later
//! sub-project.

use super::*;
use crate::asset::{AssetConsumer, AssetLoadHandle, AudioAsset};
use crate::handoff;
use crate::SignalProcessor;
use float_cmp::approx_eq;

/// Deterministic pseudo-noise in [-1, 1] (LCG; no rand dependency). Copied
/// from `tests/realtime_safety.rs` so the unit tests stay self-contained.
fn noise(len: usize, seed: u64) -> Vec<f32> {
    let mut state = seed
        .wrapping_mul(2862933555777941757)
        .wrapping_add(3037000493);
    (0..len)
        .map(|_| {
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            ((state >> 33) as f32 / (u32::MAX >> 1) as f32) - 1.0
        })
        .collect()
}

/// Crossfade length in samples at 44.1 kHz, mirroring the Convolver's
/// `prepare` computation (`CROSSFADE_SECONDS * rate`, rounded, min 1).
fn fade_len_at_44k() -> usize {
    ((CROSSFADE_SECONDS * 44100.0).round() as usize).max(1)
}

/// Compare two signals with a tolerance relative to the reference's peak.
fn assert_close_rel(got: &[f32], want: &[f32], label: &str) {
    assert_eq!(got.len(), want.len(), "{label}: length mismatch");
    let scale = want.iter().fold(1.0f32, |m, w| m.max(w.abs()));
    for (i, (&g, &w)) in got.iter().zip(want.iter()).enumerate() {
        assert!(
            approx_eq!(f32, g, w, epsilon = 1e-4 * scale),
            "{label}: index {i}: got {g}, want {w}"
        );
    }
}

fn prepared_with_ir(ir: Vec<f32>) -> Convolver {
    let mut conv = Convolver::with_ir(ir);
    conv.set_sample_rate(44100.0);
    conv.prepare();
    conv
}

/// Build a `Convolver::new()` with an installed consumer fed by `publisher`,
/// already prepared at 44.1 kHz. Returns the convolver and the publisher.
fn prepared_with_consumer() -> (Convolver, handoff::Publisher<ConvolverEngine>) {
    let (publisher, consumer) = handoff::pair::<ConvolverEngine>();
    let mut conv = Convolver::new();
    conv.install_ir_consumer(consumer);
    conv.set_sample_rate(44100.0);
    conv.prepare();
    (conv, publisher)
}

fn run(conv: &mut Convolver, input: &[f32]) -> Vec<f32> {
    input
        .iter()
        .map(|&x| {
            conv.input = x;
            conv.process();
            conv.output
        })
        .collect()
}

/// Test 1: the `with_ir` constructor and the extracted engine reproduce the
/// IR from a unit impulse (zero-latency convolver).
#[test]
fn convolver_with_ir_reproduces_impulse_response() {
    let ir = noise(1500, 1);
    let mut conv = prepared_with_ir(ir.clone());

    let mut impulse = vec![0.0f32; ir.len()];
    impulse[0] = 1.0;
    let got = run(&mut conv, &impulse);

    assert_close_rel(&got, &ir, "impulse reproduces IR");
}

/// Test 2: a published engine, taken on the (simulated) audio thread and
/// crossfaded in, matches a directly-constructed `with_ir` convolver once the
/// fade has settled.
#[test]
fn convolver_swap_matches_direct_construction() {
    let ir = noise(1500, 2);
    let asset = AudioAsset::from_samples(ir.clone(), 1, 44100, 44100).unwrap();

    let (mut conv, mut publisher) = prepared_with_consumer();
    let engine = ConvolverConsumer.build(&asset).unwrap();
    publisher.publish(engine);

    let mut reference = prepared_with_ir(ir.clone());

    let input = noise(4000, 22);
    let got = run(&mut conv, &input);
    let want = run(&mut reference, &input);

    // The new engine fades up from silence over the first `fade_len` samples,
    // so only assert equality once the fade has completed.
    let fade = fade_len_at_44k();
    assert_close_rel(
        &got[fade..],
        &want[fade..],
        "post-fade swap matches with_ir",
    );
}

/// Test 3: swapping IRs mid-stream is click-free — the largest sample-to-sample
/// jump straddling the swap is no larger than the steady-state jump.
#[test]
fn convolver_swap_is_click_free() {
    let (mut conv, mut publisher) = prepared_with_consumer();

    let a = noise(800, 3);
    let b = noise(800, 4);
    let asset_a = AudioAsset::from_samples(a, 1, 44100, 44100).unwrap();
    let asset_b = AudioAsset::from_samples(b, 1, 44100, 44100).unwrap();

    let input = noise(8192, 30);

    // Publish A, run 4096 samples.
    publisher.publish(ConvolverConsumer.build(&asset_a).unwrap());
    let mut out = run(&mut conv, &input[..4096]);
    // Publish B, run 4096 more (the swap is taken at output index 4096).
    publisher.publish(ConvolverConsumer.build(&asset_b).unwrap());
    out.extend(run(&mut conv, &input[4096..]));

    assert!(out.iter().all(|y| y.is_finite()), "all outputs finite");

    let max_jump = |range: std::ops::Range<usize>| {
        range
            .map(|i| (out[i] - out[i - 1]).abs())
            .fold(0.0f32, f32::max)
    };
    // Steady state: away from both fade windows (the first swap fades up from
    // silence over [0, fade)).
    let fade = fade_len_at_44k();
    let steady = max_jump(fade + 100..4000).max(max_jump(4096 + fade + 100..8192));
    let straddle = max_jump(4096 - 32..4096 + 32);

    assert!(
        straddle <= steady,
        "swap straddle jump {straddle} exceeds steady-state jump {steady}"
    );
}

/// Test 4: a convolver with an installed consumer but nothing published is
/// exactly silent.
#[test]
fn convolver_silent_until_first_publish() {
    let (mut conv, _publisher) = prepared_with_consumer();
    let input = noise(2048, 5);
    let got = run(&mut conv, &input);
    assert!(got.iter().all(|&y| y == 0.0), "silent until first publish");
}

/// Test 5: the `AssetLoadHandle` build → publish path drives an installed
/// convolver to the same post-fade output as `with_ir`.
#[test]
fn load_handle_publish_round_trip() {
    let ir = noise(1500, 6);
    let asset = AudioAsset::from_samples(ir.clone(), 1, 44100, 44100).unwrap();

    let (publisher, consumer) = handoff::pair::<ConvolverEngine>();
    let mut handle = AssetLoadHandle::new(publisher, ConvolverConsumer);
    handle.set_graph_rate(44100);
    handle.publish(&asset).unwrap();

    let mut conv = Convolver::new();
    conv.install_ir_consumer(consumer);
    conv.set_sample_rate(44100.0);
    conv.prepare();

    let mut reference = prepared_with_ir(ir);

    let input = noise(4000, 23);
    let got = run(&mut conv, &input);
    let want = run(&mut reference, &input);

    let fade = fade_len_at_44k();
    assert_close_rel(&got[fade..], &want[fade..], "load-handle round trip");
}

/// Test 6: `ConvolverEngine` is `Send` (it crosses the handoff).
#[test]
fn _assert_convolver_engine_send() {
    fn f<T: Send>() {}
    f::<ConvolverEngine>();
}

/// Test 7 (sub-project 4a): the `#[input(asset)]` attribute makes the derive
/// macro classify `ir` as an asset endpoint — its `EndpointAt` marker projects
/// to `AssetKind`. The macro emits no runtime endpoint-descriptor array (the
/// kind lives in the type system), so the asset recognition is asserted through
/// the marker's `Kind`. Endpoint *direction* is carried by the `#[input(...)]`
/// attribute (the field flows through the macro's input path) and is not
/// exposed at the type level.
#[test]
fn convolver_ir_is_asset_endpoint() {
    fn assert_asset_kind<N, M>()
    where
        N: crate::dispatch::EndpointAt<M, Kind = crate::dispatch::AssetKind>,
    {
    }
    assert_asset_kind::<Convolver, <Convolver>::ir__Ep>();
}
