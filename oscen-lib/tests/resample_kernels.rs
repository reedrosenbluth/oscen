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
