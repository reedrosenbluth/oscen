//! Convolution correctness tests: every implementation is compared against
//! a naive O(n*m) time-domain reference.

use oscen::convolution::{Convolver, DirectConvolver, PartitionedConvolver};
use oscen::SignalProcessor;

/// Naive direct-form convolution, truncated to the input length (matching
/// what a streaming convolver emits while the input is still flowing).
fn naive_convolve(input: &[f32], ir: &[f32]) -> Vec<f32> {
    let mut out = vec![0.0f32; input.len()];
    for (t, slot) in out.iter_mut().enumerate() {
        let mut acc = 0.0f64;
        for (k, &h) in ir.iter().enumerate() {
            if t >= k {
                acc += h as f64 * input[t - k] as f64;
            }
        }
        *slot = acc as f32;
    }
    out
}

/// Deterministic pseudo-noise in [-1, 1] (LCG; no rand dependency).
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

fn assert_close(got: &[f32], want: &[f32], epsilon: f32, label: &str) {
    assert_eq!(got.len(), want.len(), "{label}: length mismatch");
    for (i, (g, w)) in got.iter().zip(want.iter()).enumerate() {
        assert!(
            (g - w).abs() <= epsilon,
            "{label}: sample {i}: got {g}, want {w} (diff {})",
            (g - w).abs()
        );
    }
}

#[test]
fn direct_convolver_matches_naive() {
    let taps = noise(7, 1);
    let input = noise(64, 2);
    let expected = naive_convolve(&input, &taps);

    let mut conv = DirectConvolver::new(&taps);
    let got: Vec<f32> = input.iter().map(|&x| conv.process_sample(x)).collect();

    assert_close(&got, &expected, 1e-5, "direct vs naive");
}

#[test]
fn direct_convolver_single_tap_is_identity() {
    let input = noise(32, 3);
    let mut conv = DirectConvolver::new(&[1.0]);
    let got: Vec<f32> = input.iter().map(|&x| conv.process_sample(x)).collect();
    assert_close(&got, &input, 0.0, "identity");
}

#[test]
fn direct_convolver_empty_taps_is_silence() {
    let mut conv = DirectConvolver::new(&[]);
    for &x in noise(16, 4).iter() {
        assert_eq!(conv.process_sample(x), 0.0);
    }
}

#[test]
fn partitioned_convolver_matches_naive_with_one_block_latency() {
    let block_size = 8;
    let segment = noise(24, 5); // exactly 3 partitions
    let input = noise(128, 6);
    let expected = naive_convolve(&input, &segment);

    let mut conv = PartitionedConvolver::new(block_size, &segment);
    assert_eq!(conv.latency_samples(), block_size);
    let got: Vec<f32> = input.iter().map(|&x| conv.process_sample(x)).collect();

    // First `block_size` outputs are the latency zeros...
    assert_close(&got[..block_size], &vec![0.0; block_size], 1e-6, "latency");
    // ...then the convolution, delayed by one block.
    assert_close(
        &got[block_size..],
        &expected[..input.len() - block_size],
        1e-4,
        "partitioned vs naive",
    );
}

#[test]
fn partitioned_convolver_pads_non_multiple_segments() {
    let block_size = 8;
    let segment = noise(13, 7); // 2 partitions, second one padded
    let input = noise(96, 8);
    let expected = naive_convolve(&input, &segment);

    let mut conv = PartitionedConvolver::new(block_size, &segment);
    let got: Vec<f32> = input.iter().map(|&x| conv.process_sample(x)).collect();

    assert_close(
        &got[block_size..],
        &expected[..input.len() - block_size],
        1e-4,
        "padded segment",
    );
}

#[test]
fn partitioned_convolver_single_partition() {
    let block_size = 16;
    let segment = noise(5, 9); // shorter than one block
    let input = noise(80, 10);
    let expected = naive_convolve(&input, &segment);

    let mut conv = PartitionedConvolver::new(block_size, &segment);
    let got: Vec<f32> = input.iter().map(|&x| conv.process_sample(x)).collect();

    assert_close(
        &got[block_size..],
        &expected[..input.len() - block_size],
        1e-4,
        "single partition",
    );
}

#[test]
fn partitioned_convolver_empty_segment_is_silence() {
    let mut conv = PartitionedConvolver::new(8, &[]);
    for &x in noise(32, 11).iter() {
        assert_eq!(conv.process_sample(x), 0.0);
    }
}

// --- Convolver node ---

/// Drive a Convolver node standalone through the documented lifecycle
/// (set_sample_rate → prepare → per-sample process).
fn run_convolver_node(ir: &[f32], input: &[f32]) -> Vec<f32> {
    let mut node = Convolver::new(ir.to_vec());
    node.set_sample_rate(44100.0);
    node.prepare();
    input
        .iter()
        .map(|&x| {
            node.input = x;
            node.process();
            node.output
        })
        .collect()
}

fn assert_close_rel(got: &[f32], want: &[f32], label: &str) {
    let scale = want.iter().fold(1.0f32, |m, w| m.max(w.abs()));
    assert_close(got, want, 1e-4 * scale, label);
}

#[test]
fn convolver_node_matches_naive_at_zero_latency() {
    // IR lengths chosen to hit every tier boundary: head only (short and
    // exact), head + short stage, all three tiers (barely into the long
    // stage, and several long partitions deep).
    for &ir_len in &[1usize, 20, 32, 33, 100, 512, 513, 600, 1500] {
        let ir = noise(ir_len, 100 + ir_len as u64);
        let input = noise(3000, 200 + ir_len as u64);
        let expected = naive_convolve(&input, &ir);
        let got = run_convolver_node(&ir, &input);
        assert_close_rel(&got, &expected, &format!("node vs naive, ir_len={ir_len}"));
    }
}

#[test]
fn convolver_node_reproduces_ir_from_unit_impulse() {
    let ir = noise(700, 12);
    let mut input = vec![0.0f32; 1024];
    input[0] = 1.0;
    let got = run_convolver_node(&ir, &input);

    // Zero latency: the very first output sample is ir[0].
    assert!(
        (got[0] - ir[0]).abs() < 1e-5,
        "first sample: got {}, want {}",
        got[0],
        ir[0]
    );
    assert_close_rel(&got[..ir.len()], &ir, "impulse reproduces IR");
    // Past the IR, silence.
    for (i, &y) in got[ir.len()..].iter().enumerate() {
        assert!(y.abs() < 1e-3, "tail sample {i} not silent: {y}");
    }
}

#[test]
fn convolver_node_empty_ir_is_silence() {
    let input = noise(256, 13);
    let got = run_convolver_node(&[], &input);
    assert!(got.iter().all(|&y| y == 0.0));
}

#[test]
fn convolver_node_prepare_is_idempotent() {
    let ir = noise(600, 14);
    let input = noise(2048, 15);

    let once = run_convolver_node(&ir, &input);

    let mut node = Convolver::new(ir.to_vec());
    node.set_sample_rate(44100.0);
    node.prepare();
    node.prepare();
    let twice: Vec<f32> = input
        .iter()
        .map(|&x| {
            node.input = x;
            node.process();
            node.output
        })
        .collect();

    assert_close(&once, &twice, 0.0, "prepare idempotence");
}
