//! Realtime-safety tests: the convolution audio path must never allocate.
//!
//! `AllocDisabler` is installed as the global allocator for this test
//! binary; any heap allocation inside an `assert_no_alloc` region aborts
//! the test. (The checks are active in debug builds, which is how `cargo
//! test` runs by default.)

use assert_no_alloc::{assert_no_alloc, AllocDisabler};
use oscen::convolution::{Convolver, DirectConvolver, PartitionedConvolver};
use oscen::spectral::FftPlan;
use oscen::SignalProcessor;

#[global_allocator]
static ALLOC: AllocDisabler = AllocDisabler;

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

#[test]
fn convolver_node_process_does_not_allocate() {
    // IR long enough to engage all three tiers (head, short, long stage).
    let ir = noise(1500, 1);
    let input = noise(2048, 2);

    let mut node = Convolver::new(ir);
    node.set_sample_rate(44100.0);
    node.prepare();

    // 2048 samples crosses both the 32- and 512-sample block boundaries
    // many times, including the samples where both stages fire at once.
    let sum = assert_no_alloc(|| {
        let mut sum = 0.0f32;
        for &x in &input {
            node.input = x;
            node.process();
            sum += node.output;
        }
        sum
    });
    assert!(sum.is_finite());
}

#[test]
fn convolver_structs_process_does_not_allocate() {
    let input = noise(1024, 3);

    let mut direct = DirectConvolver::new(&noise(32, 4));
    let mut partitioned = PartitionedConvolver::new(64, &noise(300, 5));

    let sum = assert_no_alloc(|| {
        let mut sum = 0.0f32;
        for &x in &input {
            sum += direct.process_sample(x) + partitioned.process_sample(x);
        }
        sum
    });
    assert!(sum.is_finite());
}

#[test]
fn fft_plan_forward_inverse_does_not_allocate() {
    let mut plan = FftPlan::new(1024);
    let mut time = noise(1024, 6);
    let mut spectrum = plan.make_spectrum();
    let mut output = vec![0.0f32; 1024];

    assert_no_alloc(|| {
        plan.forward(&mut time, &mut spectrum);
        plan.inverse(&mut spectrum, &mut output);
    });
    assert!(output.iter().all(|y| y.is_finite()));
}
