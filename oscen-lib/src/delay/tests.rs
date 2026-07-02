use super::*;
use float_cmp::assert_approx_eq;

/// Build a prepared `Delay` (rate distributed before `prepare`, as a graph
/// would do it).
fn prepared(delay_samples: f32, feedback: f32, sample_rate: f32) -> Delay {
    let mut delay = Delay::new(delay_samples, feedback);
    delay.set_sample_rate(sample_rate);
    delay.prepare();
    delay
}

/// Drive the node one sample at a time, returning the outputs.
fn run(delay: &mut Delay, input: impl Iterator<Item = f32>) -> Vec<f32> {
    input
        .map(|x| {
            delay.input = x;
            delay.process();
            delay.output
        })
        .collect()
}

/// An impulse through a commanded integer delay of N comes out as a single
/// unscaled impulse exactly N + 1 process calls later (the node reads before
/// it writes, so the commanded delay is measured from the previous sample).
#[test]
fn impulse_is_delayed_by_commanded_integer_samples() {
    let n = 100;
    let mut delay = prepared(n as f32, 0.0, 44_100.0);

    let out = run(&mut delay, (0..300).map(|t| if t == 0 { 1.0 } else { 0.0 }));

    for (t, &y) in out.iter().enumerate() {
        let want = if t == n + 1 { 1.0 } else { 0.0 };
        assert_approx_eq!(f32, y, want, epsilon = 0.0);
    }
}

/// A commanded delay of zero is a one-sample delay (read-before-write): the
/// output is exactly the previous input, passed through untouched.
#[test]
fn zero_delay_is_a_one_sample_identity() {
    let mut delay = prepared(0.0, 0.0, 44_100.0);

    let input: Vec<f32> = (0..200)
        .map(|t| ((t * 7 % 23) as f32) * 0.05 - 0.5)
        .collect();
    let out = run(&mut delay, input.iter().copied());

    assert_approx_eq!(f32, out[0], 0.0, epsilon = 0.0);
    for t in 1..input.len() {
        assert_approx_eq!(f32, out[t], input[t - 1], epsilon = 0.0);
    }
}

/// With feedback 0.9 an impulse produces a geometrically decaying echo train
/// that stays bounded; between echoes the output is exactly silent.
#[test]
fn feedback_echoes_decay_and_stay_bounded() {
    let n = 10;
    let mut delay = prepared(n as f32, 0.9, 44_100.0);

    let out = run(
        &mut delay,
        (0..1_000).map(|t| if t == 0 { 1.0 } else { 0.0 }),
    );

    assert!(out.iter().all(|y| y.abs() <= 1.0), "output must be bounded");
    // Echo k arrives at t = k * (n + 1) with amplitude 0.9^(k - 1).
    for k in 1..=5usize {
        let t = k * (n + 1);
        let want = 0.9f32.powi(k as i32 - 1);
        assert_approx_eq!(f32, out[t], want, epsilon = 1e-6);
        assert_approx_eq!(f32, out[t - 1], 0.0, epsilon = 0.0);
        assert_approx_eq!(f32, out[t + 1], 0.0, epsilon = 0.0);
    }
}

/// Processing more samples than the buffer holds keeps the output correct
/// across the write-position wrap-around.
#[test]
fn output_stays_correct_across_buffer_wrap_around() {
    // 2 s at 1 kHz -> 2000 requested, 2048 actual capacity; 5000 samples
    // wraps the buffer more than twice.
    let n = 100;
    let mut delay = prepared(n as f32, 0.0, 1_000.0);
    assert_eq!(delay.buffer.capacity(), 2048);

    let input: Vec<f32> = (0..5_000).map(|t| ((t % 97) as f32) * 0.01).collect();
    let out = run(&mut delay, input.iter().copied());

    for t in (n + 1)..input.len() {
        assert_approx_eq!(f32, out[t], input[t - n - 1], epsilon = 0.0);
    }
}

/// A fractional delay below one sample must never read the oldest sample in
/// the buffer: once the impulse has aged to the write position, the output
/// stays exactly silent (regression for the cubic write-boundary bleed).
#[test]
fn fractional_delay_below_one_sample_has_no_stale_bleed() {
    let mut delay = prepared(0.5, 0.0, 1_000.0);
    let capacity = delay.buffer.capacity();

    // Impulse, then silence for over a full buffer length. The interpolated
    // impulse leaves the output within two samples; at t = capacity the
    // impulse sits at the write position (the oldest sample), where a cubic
    // read at offset 0.5 used to pick it up at weight -0.0625.
    let total = capacity + 16;
    let out = run(
        &mut delay,
        (0..total).map(|t| if t == 0 { 1.0 } else { 0.0 }),
    );

    assert_approx_eq!(f32, out[1], 0.5, epsilon = 1e-6);
    assert_approx_eq!(f32, out[2], 0.5, epsilon = 1e-6);
    assert!(
        out[3..].iter().all(|&y| y == 0.0),
        "stale sample bled into a sub-sample delay: {:?}",
        out[3..].iter().find(|&&y| y != 0.0)
    );
}

/// A decaying feedback tail must be flushed to zero instead of recirculating
/// through the f32 subnormal range after the input stops.
#[test]
fn feedback_tail_flushes_denormals() {
    let mut delay = prepared(4.0, 0.9, 44_100.0);

    // One impulse, then silence. The tail decays by 0.9 per 5-sample round
    // trip and would reach subnormal magnitudes (~1e-38) after roughly 830
    // trips without the flush.
    let out = run(
        &mut delay,
        (0..6_000).map(|t| if t == 0 { 1.0 } else { 0.0 }),
    );

    assert!(
        out.iter().all(|y| !y.is_subnormal()),
        "output entered the subnormal range"
    );
    // The flush snaps the tail to exactly zero once it falls below the
    // threshold; the final stretch must be dead silence.
    assert!(
        out[5_000..].iter().all(|&y| y == 0.0),
        "tail did not settle to exact zero"
    );
}

/// The maximum delay is fixed in *time* (2 s), not in samples: a 1.5 s delay
/// must survive at 96 kHz instead of being silently clamped to a
/// 44.1 kHz-sized buffer.
#[test]
fn max_delay_is_sample_rate_independent() {
    let n = 144_000; // 1.5 s at 96 kHz
    let mut delay = prepared(n as f32, 0.0, 96_000.0);

    let total = n + 100;
    let out = run(
        &mut delay,
        (0..total).map(|t| if t == 0 { 1.0 } else { 0.0 }),
    );

    let first_nonzero = out.iter().position(|&y| y != 0.0);
    // The node reads before it writes, so a commanded delay of N samples
    // emerges N + 1 process calls after the input.
    assert_eq!(
        first_nonzero,
        Some(n + 1),
        "1.5 s delay at 96 kHz must not be clamped"
    );
}
