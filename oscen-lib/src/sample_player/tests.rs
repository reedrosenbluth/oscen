//! Unit tests for `SamplePlayer`: in-order looping playback, playhead reset on
//! swap, and silence when no buffer has been published. Buffers are fed
//! directly through a `handoff::pair` (bypassing WAV decode); comparisons use
//! `float_cmp::approx_eq` (not `==`).

use super::*;
use crate::asset::AudioAsset;
use crate::frame::{AudioFrame, Frame};
use crate::handoff;
use crate::SignalProcessor;
use float_cmp::approx_eq;

const RATE: u32 = 44_100;

/// Drive `player` for `n` samples, collecting `output` each call.
fn run<F: AudioFrame>(player: &mut SamplePlayer<F>, n: usize) -> Vec<F> {
    (0..n)
        .map(|_| {
            player.process();
            player.output
        })
        .collect()
}

fn assert_close(got: &[f32], want: &[f32], label: &str) {
    assert_eq!(got.len(), want.len(), "{label}: length mismatch");
    for (i, (&g, &w)) in got.iter().zip(want.iter()).enumerate() {
        assert!(
            approx_eq!(f32, g, w, epsilon = 1e-6),
            "{label}: index {i}: got {g}, want {w}"
        );
    }
}

/// A published buffer is played in order and then loops.
#[test]
fn plays_buffer_in_order_then_loops() {
    let (mut publisher, consumer) = handoff::pair::<Vec<f32>>();
    let mut player = SamplePlayer::new();
    player.install_buf_consumer(consumer);

    publisher.publish(vec![0.1, 0.2, 0.3]);

    let got = run(&mut player, 7);
    assert_close(&got, &[0.1, 0.2, 0.3, 0.1, 0.2, 0.3, 0.1], "loop playback");
}

/// Publishing a new buffer mid-playback resets the playhead to 0.
#[test]
fn swap_resets_playhead() {
    let (mut publisher, consumer) = handoff::pair::<Vec<f32>>();
    let mut player = SamplePlayer::new();
    player.install_buf_consumer(consumer);

    publisher.publish(vec![0.1, 0.2, 0.3, 0.4]);
    let first = run(&mut player, 2);
    assert_close(&first, &[0.1, 0.2], "before swap");

    publisher.publish(vec![0.8, 0.9]);
    let second = run(&mut player, 3);
    assert_close(&second, &[0.8, 0.9, 0.8], "after swap resets playhead");
}

/// A player with no buffer published is exactly silent and does not panic.
#[test]
fn silent_when_unloaded() {
    let mut player = SamplePlayer::<f32>::new();
    let got = run(&mut player, 4);
    assert!(got.iter().all(|&y| y == 0.0), "silent when unloaded");
}

/// Build a `Vec<F>` playable from an interleaved buffer via the channel-major
/// asset and the node's own consumer (exercises the channel-mapping rule).
fn build_playable<F: AudioFrame>(interleaved: Vec<f32>, channels: usize) -> Vec<F> {
    let asset = AudioAsset::from_samples(interleaved, channels, RATE, RATE).expect("build asset");
    SamplePlayerConsumer::<F>::default()
        .build(&asset)
        .expect("build playable")
}

/// A stereo source plays each channel independently and loops; distinct L/R
/// rules out a broadcast or channel-zeroing bug.
#[test]
fn stereo_plays_per_channel_then_loops() {
    // L = [0.1, 0.2, 0.3], R = [-0.1, -0.2, -0.3]; interleaved frame-major.
    let interleaved = vec![0.1, -0.1, 0.2, -0.2, 0.3, -0.3];
    let playable = build_playable::<Frame<2>>(interleaved, 2);

    let (mut publisher, consumer) = handoff::pair::<Vec<Frame<2>>>();
    let mut player = SamplePlayer::<Frame<2>>::new();
    player.install_buf_consumer(consumer);
    publisher.publish(playable);

    let got = run(&mut player, 7);
    let want = [
        Frame([0.1, -0.1]),
        Frame([0.2, -0.2]),
        Frame([0.3, -0.3]),
        Frame([0.1, -0.1]),
        Frame([0.2, -0.2]),
        Frame([0.3, -0.3]),
        Frame([0.1, -0.1]),
    ];
    assert_eq!(got.len(), want.len());
    for (i, (g, w)) in got.iter().zip(want.iter()).enumerate() {
        assert!(
            approx_eq!(f32, g.0[0], w.0[0], epsilon = 1e-6)
                && approx_eq!(f32, g.0[1], w.0[1], epsilon = 1e-6),
            "stereo playback at {i}: got {g:?}, want {w:?}"
        );
    }
}

/// A mono source played by a stereo player broadcasts to both channels.
#[test]
fn mono_source_broadcasts_to_stereo() {
    let mono = vec![0.1, 0.2, 0.3];
    let playable = build_playable::<Frame<2>>(mono.clone(), 1);

    let (mut publisher, consumer) = handoff::pair::<Vec<Frame<2>>>();
    let mut player = SamplePlayer::<Frame<2>>::new();
    player.install_buf_consumer(consumer);
    publisher.publish(playable);

    let got = run(&mut player, 3);
    for (i, (g, &s)) in got.iter().zip(mono.iter()).enumerate() {
        assert!(
            approx_eq!(f32, g.0[0], s, epsilon = 1e-6)
                && approx_eq!(f32, g.0[1], s, epsilon = 1e-6),
            "mono->stereo broadcast at {i}: got {g:?}, want Frame([{s}, {s}])"
        );
    }
}

/// Deviation pin: a multi-channel source played by a mono `SamplePlayer<f32>`
/// takes channel 0 (left), NOT a downmix average of all channels. The generic
/// `build` reduces to "copy channel 0" for `f32` (`F::CHANNELS == 1`).
#[test]
fn multichannel_source_to_mono_takes_channel_zero() {
    // L = [0.1, 0.2], R = [0.9, 0.8]; a downmix would yield [0.5, 0.5].
    let interleaved = vec![0.1, 0.9, 0.2, 0.8];
    let playable = build_playable::<f32>(interleaved, 2);

    let (mut publisher, consumer) = handoff::pair::<Vec<f32>>();
    let mut player = SamplePlayer::<f32>::new();
    player.install_buf_consumer(consumer);
    publisher.publish(playable);

    let got = run(&mut player, 2);
    assert_close(
        &got,
        &[0.1, 0.2],
        "f32 multi-channel source takes channel 0",
    );
}
