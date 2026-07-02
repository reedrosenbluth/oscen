use super::*;

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
