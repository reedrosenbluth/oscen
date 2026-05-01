use oscen::resample::{LatchDown, LatchUp, StreamDownsampler, StreamUpsampler};

#[test]
fn latch_up_2_holds_value() {
    let mut up = LatchUp::<2>::new();
    let mut out = [0.0_f32; 2];
    up.upsample(1.0, &mut out);
    assert_eq!(out, [1.0, 1.0]);
    up.upsample(-0.25, &mut out);
    assert_eq!(out, [-0.25, -0.25]);
}

#[test]
fn latch_up_4_holds_value() {
    let mut up = LatchUp::<4>::new();
    let mut out = [0.0_f32; 4];
    up.upsample(0.5, &mut out);
    assert_eq!(out, [0.5; 4]);
}

#[test]
fn latch_up_zero_latency() {
    assert_eq!(LatchUp::<2>::new().latency_samples(), 0);
    assert_eq!(LatchUp::<4>::new().latency_samples(), 0);
    assert_eq!(LatchUp::<8>::new().latency_samples(), 0);
}

#[test]
fn latch_down_2_takes_first() {
    let mut down = LatchDown::<2>::new();
    let y = down.downsample(&[1.0, 2.0]);
    assert_eq!(y, 1.0);
    let y = down.downsample(&[3.0, 4.0]);
    assert_eq!(y, 3.0);
}

#[test]
fn latch_down_4_takes_first() {
    let mut down = LatchDown::<4>::new();
    let y = down.downsample(&[10.0, 11.0, 12.0, 13.0]);
    assert_eq!(y, 10.0);
}

#[test]
fn latch_down_zero_latency() {
    assert_eq!(LatchDown::<2>::new().latency_samples(), 0);
    assert_eq!(LatchDown::<4>::new().latency_samples(), 0);
    assert_eq!(LatchDown::<8>::new().latency_samples(), 0);
}

#[test]
fn latch_reset_is_noop() {
    let mut up = LatchUp::<2>::new();
    up.reset();
    let mut out = [0.0; 2];
    up.upsample(1.0, &mut out);
    assert_eq!(out, [1.0, 1.0]);
}

use oscen::resample::{LinearDown, LinearUp};
use float_cmp::approx_eq;

#[test]
fn linear_up_2_interpolates_between_samples() {
    let mut up = LinearUp::<2>::new();
    let mut out = [0.0_f32; 2];

    // Sentinel definition: out[i] = prev + (current - prev) * (i / N) for i in 0..N.
    up.upsample(0.0, &mut out);
    up.upsample(1.0, &mut out);
    // After feeding 0.0 then 1.0, out should be [0.0, 0.5]
    assert!(approx_eq!(f32, out[0], 0.0, epsilon = 1e-6));
    assert!(approx_eq!(f32, out[1], 0.5, epsilon = 1e-6));

    up.upsample(2.0, &mut out);
    // prev=1.0, current=2.0, N=2: out[0] = 1.0, out[1] = 1.5.
    assert!(approx_eq!(f32, out[0], 1.0, epsilon = 1e-6));
    assert!(approx_eq!(f32, out[1], 1.5, epsilon = 1e-6));
}

#[test]
fn linear_up_dc_passes_through_after_warmup() {
    let mut up = LinearUp::<4>::new();
    let mut out = [0.0_f32; 4];
    // After two consecutive identical inputs, the output should be the constant.
    up.upsample(0.7, &mut out);
    up.upsample(0.7, &mut out);
    for v in out {
        assert!(approx_eq!(f32, v, 0.7, epsilon = 1e-6));
    }
}

#[test]
fn linear_up_latency_is_one_dest_sample() {
    assert_eq!(LinearUp::<2>::new().latency_samples(), 1);
    assert_eq!(LinearUp::<4>::new().latency_samples(), 1);
}

#[test]
fn linear_down_2_averages_pair() {
    let mut down = LinearDown::<2>::new();
    let y = down.downsample(&[1.0, 3.0]);
    assert!(approx_eq!(f32, y, 2.0, epsilon = 1e-6));
}

#[test]
fn linear_down_4_averages() {
    let mut down = LinearDown::<4>::new();
    let y = down.downsample(&[1.0, 2.0, 3.0, 4.0]);
    assert!(approx_eq!(f32, y, 2.5, epsilon = 1e-6));
}

#[test]
fn linear_reset_clears_history() {
    let mut up = LinearUp::<2>::new();
    let mut out = [0.0_f32; 2];
    up.upsample(1.0, &mut out);
    up.reset();
    up.upsample(1.0, &mut out);
    // After reset prev should be 0 again, so out[0] = 0.0, out[1] = 0.5
    assert!(approx_eq!(f32, out[0], 0.0, epsilon = 1e-6));
    assert!(approx_eq!(f32, out[1], 0.5, epsilon = 1e-6));
}
