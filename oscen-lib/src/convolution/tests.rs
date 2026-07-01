//! Tests for the buildable `ConvolverEngine`, the live IR swap, and the
//! equal-power crossfade. The IR-swap path is exercised by hand-building a
//! `handoff::pair` and an `AssetLoadHandle`; the `graph!` wiring is a later
//! sub-project.

use super::*;
use crate::asset::{AssetConsumer, AssetLoadHandle, AudioAsset};
use crate::frame::Frame;
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
fn prepared_with_consumer() -> (Convolver, handoff::Publisher<MultiConvolverEngine>) {
    let (publisher, consumer) = handoff::pair::<MultiConvolverEngine>();
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
    let engine = ConvolverConsumer::<f32>::default().build(&asset).unwrap();
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
    publisher.publish(ConvolverConsumer::<f32>::default().build(&asset_a).unwrap());
    let mut out = run(&mut conv, &input[..4096]);
    // Publish B, run 4096 more (the swap is taken at output index 4096).
    publisher.publish(ConvolverConsumer::<f32>::default().build(&asset_b).unwrap());
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

    let (publisher, consumer) = handoff::pair::<MultiConvolverEngine>();
    let mut handle = AssetLoadHandle::new(publisher, ConvolverConsumer::<f32>::default());
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

/// Drive a `MultiConvolverEngine` with a sequence of stereo input frames,
/// returning the per-channel output as `(left, right)` vectors.
fn run_stereo(engine: &mut MultiConvolverEngine, input: &[Frame<2>]) -> (Vec<f32>, Vec<f32>) {
    let frames: Vec<Frame<2>> = input.iter().map(|&x| engine.process_frame(x)).collect();
    let left = frames.iter().map(|f| f.0[0]).collect();
    let right = frames.iter().map(|f| f.0[1]).collect();
    (left, right)
}

/// A stereo impulse on one channel reproduces that channel's IR and leaves the
/// other channel silent: per-channel convolution with no L↔R bleed.
#[test]
fn multi_convolver_engine_per_channel_no_bleed() {
    let ir_l = vec![0.5f32, -0.25, 0.125];
    let ir_r = vec![0.2f32, 0.4, -0.1];
    // Channel-major asset: build interleaved L,R,L,R,... for `from_samples`.
    let interleaved: Vec<f32> = ir_l
        .iter()
        .zip(ir_r.iter())
        .flat_map(|(&l, &r)| [l, r])
        .collect();
    let asset = AudioAsset::from_samples(interleaved, 2, 44100, 44100).unwrap();

    // Impulse on the LEFT channel only.
    let mut engine = ConvolverConsumer::<Frame<2>>::default()
        .build(&asset)
        .unwrap();
    let mut input = vec![Frame([0.0, 0.0]); ir_l.len()];
    input[0] = Frame([1.0, 0.0]);
    let (left, right) = run_stereo(&mut engine, &input);
    assert_close_rel(&left, &ir_l, "left impulse reproduces IR-left");
    assert!(
        right.iter().all(|&y| y == 0.0),
        "left impulse must not bleed into right: {right:?}"
    );

    // Impulse on the RIGHT channel only (fresh engine).
    let mut engine = ConvolverConsumer::<Frame<2>>::default()
        .build(&asset)
        .unwrap();
    let mut input = vec![Frame([0.0, 0.0]); ir_r.len()];
    input[0] = Frame([0.0, 1.0]);
    let (left, right) = run_stereo(&mut engine, &input);
    assert_close_rel(&right, &ir_r, "right impulse reproduces IR-right");
    assert!(
        left.iter().all(|&y| y == 0.0),
        "right impulse must not bleed into left: {left:?}"
    );
}

/// A 1-channel IR drives a 2-channel engine by broadcasting: every output
/// channel reproduces the mono IR of its own input channel.
#[test]
fn multi_convolver_engine_mono_ir_broadcasts() {
    let ir = vec![0.3f32, -0.6, 0.2];
    let asset = AudioAsset::from_samples(ir.clone(), 1, 44100, 44100).unwrap();

    let mut engine = ConvolverConsumer::<Frame<2>>::default()
        .build(&asset)
        .unwrap();
    assert_eq!(engine.num_channels(), 2, "F::CHANNELS engines built");

    // Impulse on both channels: each output channel reproduces the broadcast IR.
    let mut input = vec![Frame([0.0, 0.0]); ir.len()];
    input[0] = Frame([1.0, 1.0]);
    let (left, right) = run_stereo(&mut engine, &input);
    assert_close_rel(&left, &ir, "mono IR broadcast to left");
    assert_close_rel(&right, &ir, "mono IR broadcast to right");
}

/// A multi-channel IR into a mono engine averages the channels, matching the
/// old `from_wav` downmix instead of silently taking channel 0.
#[test]
fn multi_convolver_engine_stereo_ir_to_mono_averages() {
    let ir_l = vec![0.5f32, -0.25, 0.125];
    let ir_r = vec![0.2f32, 0.4, -0.1];
    let averaged: Vec<f32> = ir_l
        .iter()
        .zip(ir_r.iter())
        .map(|(&l, &r)| (l + r) * 0.5)
        .collect();
    let interleaved: Vec<f32> = ir_l
        .iter()
        .zip(ir_r.iter())
        .flat_map(|(&l, &r)| [l, r])
        .collect();
    let asset = AudioAsset::from_samples(interleaved, 2, 44100, 44100).unwrap();

    let mut engine = ConvolverConsumer::<f32>::default().build(&asset).unwrap();
    assert_eq!(engine.num_channels(), 1);

    let mut input = vec![0.0f32; averaged.len()];
    input[0] = 1.0;
    let got: Vec<f32> = input.iter().map(|&x| engine.process_frame(x)).collect();
    assert_close_rel(&got, &averaged, "stereo IR averaged into mono engine");
}

/// A stereo input sequence with an impulse on `channel` at t=0, then zeros.
fn stereo_impulse(channel: usize, len: usize) -> Vec<Frame<2>> {
    let mut input = vec![Frame([0.0, 0.0]); len];
    input[0].0[channel] = 1.0;
    input
}

/// Drive a stereo `Convolver<Frame<2>>` node, returning per-channel output.
fn run_stereo_node(conv: &mut Convolver<Frame<2>>, input: &[Frame<2>]) -> (Vec<f32>, Vec<f32>) {
    let frames: Vec<Frame<2>> = input
        .iter()
        .map(|&x| {
            conv.input = x;
            conv.process();
            conv.output
        })
        .collect();
    let left = frames.iter().map(|f| f.0[0]).collect();
    let right = frames.iter().map(|f| f.0[1]).collect();
    (left, right)
}

/// Build a stereo `Convolver<Frame<2>>` with a per-channel IR published through
/// the asset path and faded fully in (silent input flushes engine history to
/// zero), so a subsequent impulse reproduces the IR at full gain.
fn prepared_stereo_with_ir(ir_l: &[f32], ir_r: &[f32]) -> Convolver<Frame<2>> {
    let interleaved: Vec<f32> = ir_l.iter().zip(ir_r).flat_map(|(&l, &r)| [l, r]).collect();
    let asset = AudioAsset::from_samples(interleaved, 2, 44100, 44100).unwrap();

    let (mut publisher, consumer) = handoff::pair::<MultiConvolverEngine>();
    let mut conv = Convolver::<Frame<2>>::new();
    conv.install_ir_consumer(consumer);
    conv.set_sample_rate(44100.0);
    conv.prepare();
    publisher.publish(
        ConvolverConsumer::<Frame<2>>::default()
            .build(&asset)
            .unwrap(),
    );

    // Fade the new engine fully in with silent input; the crossfade completes
    // and the engines' histories settle back to zero.
    for _ in 0..fade_len_at_44k() + 1 {
        conv.input = Frame([0.0, 0.0]);
        conv.process();
    }
    conv
}

/// The generic node at `Frame<2>` convolves each channel with its own IR and
/// does not bleed L↔R: a post-fade impulse on one channel reproduces that
/// channel's IR while the other channel stays silent.
#[test]
fn convolver_stereo_node_reproduces_per_channel_ir() {
    let ir_l = vec![0.5f32, -0.25, 0.125];
    let ir_r = vec![0.2f32, 0.4, -0.1];

    // Left impulse -> left IR, right silent.
    let mut conv = prepared_stereo_with_ir(&ir_l, &ir_r);
    let (l, r) = run_stereo_node(&mut conv, &stereo_impulse(0, ir_l.len()));
    assert_close_rel(&l, &ir_l, "stereo node left reproduces IR-left");
    assert!(r.iter().all(|&y| y == 0.0), "no left->right bleed: {r:?}");

    // Right impulse -> right IR, left silent (fresh node).
    let mut conv = prepared_stereo_with_ir(&ir_l, &ir_r);
    let (l, r) = run_stereo_node(&mut conv, &stereo_impulse(1, ir_r.len()));
    assert_close_rel(&r, &ir_r, "stereo node right reproduces IR-right");
    assert!(l.iter().all(|&y| y == 0.0), "no right->left bleed: {l:?}");
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
