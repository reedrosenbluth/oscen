//! Unit tests for `SamplePlayer`: in-order looping playback, playhead reset on
//! swap, and silence when no buffer has been published. Buffers are fed
//! directly through a `handoff::pair` (bypassing WAV decode); comparisons use
//! `float_cmp::approx_eq` (not `==`).

use super::*;
use crate::handoff;
use crate::SignalProcessor;
use float_cmp::approx_eq;

/// Drive `player` for `n` samples, collecting `output` each call.
fn run(player: &mut SamplePlayer, n: usize) -> Vec<f32> {
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
    let mut player = SamplePlayer::new();
    let got = run(&mut player, 4);
    assert!(got.iter().all(|&y| y == 0.0), "silent when unloaded");
}
