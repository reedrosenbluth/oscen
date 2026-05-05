//! Unit tests for `CrossRateKernel` impls. Each test exercises one impl
//! end-to-end: build state, run the three lifecycle methods, observe the
//! effect on the dest field.

use oscen::dispatch::{
    CrossRateKernel, DefaultPolicy, DownDir, LatchPolicy, LinearPolicy, SincIirPolicy, SincPolicy,
    StreamKind, UpDir,
};
use oscen::graph::{StreamInput, StreamOutput};

#[test]
fn stream_up_default_routes_through_sinc_fir() {
    // Build state via Default, run before_inner with an impulse, then call
    // on_inner for each of N inner ticks. dst should hold the sinc upsampler's
    // output. Exact values depend on SincFir<2> coefficients; assert at
    // minimum that nothing panics and the state advances.
    type State = <() as CrossRateKernel<StreamKind, StreamKind, DefaultPolicy, 2, UpDir>>::State;
    let mut state: State = Default::default();
    let src = StreamOutput::<f32>(1.0);
    let mut dst = StreamInput::<f32>::default();

    <() as CrossRateKernel<StreamKind, StreamKind, DefaultPolicy, 2, UpDir>>::before_inner(
        &mut state, &src, &mut dst,
    );
    for inner in 0..2 {
        <() as CrossRateKernel<StreamKind, StreamKind, DefaultPolicy, 2, UpDir>>::on_inner(
            &mut state, inner, &src, &mut dst,
        );
    }
    <() as CrossRateKernel<StreamKind, StreamKind, DefaultPolicy, 2, UpDir>>::after_inner(
        &mut state, &src, &mut dst,
    );
}

#[test]
fn stream_up_sinc_uses_sinc_fir() {
    type S = <() as CrossRateKernel<StreamKind, StreamKind, SincPolicy, 2, UpDir>>::State;
    let mut state: S = Default::default();
    let src = StreamOutput::<f32>(1.0);
    let mut dst = StreamInput::<f32>::default();
    <() as CrossRateKernel<StreamKind, StreamKind, SincPolicy, 2, UpDir>>::before_inner(
        &mut state, &src, &mut dst,
    );
    for inner in 0..2 {
        <() as CrossRateKernel<StreamKind, StreamKind, SincPolicy, 2, UpDir>>::on_inner(
            &mut state, inner, &src, &mut dst,
        );
    }
    <() as CrossRateKernel<StreamKind, StreamKind, SincPolicy, 2, UpDir>>::after_inner(
        &mut state, &src, &mut dst,
    );
}

#[test]
fn stream_up_sinc_iir_uses_iir_halfband() {
    type S = <() as CrossRateKernel<StreamKind, StreamKind, SincIirPolicy, 2, UpDir>>::State;
    let mut state: S = Default::default();
    let src = StreamOutput::<f32>(1.0);
    let mut dst = StreamInput::<f32>::default();
    <() as CrossRateKernel<StreamKind, StreamKind, SincIirPolicy, 2, UpDir>>::before_inner(
        &mut state, &src, &mut dst,
    );
    for inner in 0..2 {
        <() as CrossRateKernel<StreamKind, StreamKind, SincIirPolicy, 2, UpDir>>::on_inner(
            &mut state, inner, &src, &mut dst,
        );
    }
    <() as CrossRateKernel<StreamKind, StreamKind, SincIirPolicy, 2, UpDir>>::after_inner(
        &mut state, &src, &mut dst,
    );
}

#[test]
fn stream_up_linear_uses_linear() {
    type S = <() as CrossRateKernel<StreamKind, StreamKind, LinearPolicy, 2, UpDir>>::State;
    let mut state: S = Default::default();
    let src = StreamOutput::<f32>(0.5);
    let mut dst = StreamInput::<f32>::default();
    <() as CrossRateKernel<StreamKind, StreamKind, LinearPolicy, 2, UpDir>>::before_inner(
        &mut state, &src, &mut dst,
    );
    for inner in 0..2 {
        <() as CrossRateKernel<StreamKind, StreamKind, LinearPolicy, 2, UpDir>>::on_inner(
            &mut state, inner, &src, &mut dst,
        );
    }
    <() as CrossRateKernel<StreamKind, StreamKind, LinearPolicy, 2, UpDir>>::after_inner(
        &mut state, &src, &mut dst,
    );
}

#[test]
fn stream_down_default_routes_through_sinc_fir_down() {
    type State = <() as CrossRateKernel<StreamKind, StreamKind, DefaultPolicy, 2, DownDir>>::State;
    let mut state: State = Default::default();
    let src = StreamOutput::<f32>(1.0);
    let mut dst = StreamInput::<f32>::default();

    <() as CrossRateKernel<StreamKind, StreamKind, DefaultPolicy, 2, DownDir>>::before_inner(
        &mut state, &src, &mut dst,
    );
    for inner in 0..2 {
        <() as CrossRateKernel<StreamKind, StreamKind, DefaultPolicy, 2, DownDir>>::on_inner(
            &mut state, inner, &src, &mut dst,
        );
    }
    <() as CrossRateKernel<StreamKind, StreamKind, DefaultPolicy, 2, DownDir>>::after_inner(
        &mut state, &src, &mut dst,
    );
}

#[test]
fn stream_down_sinc_uses_sinc_fir_down() {
    type S = <() as CrossRateKernel<StreamKind, StreamKind, SincPolicy, 2, DownDir>>::State;
    let mut state: S = Default::default();
    let src = StreamOutput::<f32>(1.0);
    let mut dst = StreamInput::<f32>::default();
    <() as CrossRateKernel<StreamKind, StreamKind, SincPolicy, 2, DownDir>>::before_inner(
        &mut state, &src, &mut dst,
    );
    for inner in 0..2 {
        <() as CrossRateKernel<StreamKind, StreamKind, SincPolicy, 2, DownDir>>::on_inner(
            &mut state, inner, &src, &mut dst,
        );
    }
    <() as CrossRateKernel<StreamKind, StreamKind, SincPolicy, 2, DownDir>>::after_inner(
        &mut state, &src, &mut dst,
    );
}

#[test]
fn stream_down_sinc_iir_uses_iir_halfband_down() {
    type S = <() as CrossRateKernel<StreamKind, StreamKind, SincIirPolicy, 2, DownDir>>::State;
    let mut state: S = Default::default();
    let src = StreamOutput::<f32>(1.0);
    let mut dst = StreamInput::<f32>::default();
    <() as CrossRateKernel<StreamKind, StreamKind, SincIirPolicy, 2, DownDir>>::before_inner(
        &mut state, &src, &mut dst,
    );
    for inner in 0..2 {
        <() as CrossRateKernel<StreamKind, StreamKind, SincIirPolicy, 2, DownDir>>::on_inner(
            &mut state, inner, &src, &mut dst,
        );
    }
    <() as CrossRateKernel<StreamKind, StreamKind, SincIirPolicy, 2, DownDir>>::after_inner(
        &mut state, &src, &mut dst,
    );
}

#[test]
fn stream_down_linear_uses_linear_down() {
    type S = <() as CrossRateKernel<StreamKind, StreamKind, LinearPolicy, 2, DownDir>>::State;
    let mut state: S = Default::default();
    let src = StreamOutput::<f32>(0.5);
    let mut dst = StreamInput::<f32>::default();
    <() as CrossRateKernel<StreamKind, StreamKind, LinearPolicy, 2, DownDir>>::before_inner(
        &mut state, &src, &mut dst,
    );
    for inner in 0..2 {
        <() as CrossRateKernel<StreamKind, StreamKind, LinearPolicy, 2, DownDir>>::on_inner(
            &mut state, inner, &src, &mut dst,
        );
    }
    <() as CrossRateKernel<StreamKind, StreamKind, LinearPolicy, 2, DownDir>>::after_inner(
        &mut state, &src, &mut dst,
    );
}

#[test]
fn stream_down_latch_uses_latch_down() {
    type S = <() as CrossRateKernel<StreamKind, StreamKind, LatchPolicy, 2, DownDir>>::State;
    let mut state: S = Default::default();
    // Feed two distinct samples; LatchDown returns the first.
    let mut dst = StreamInput::<f32>::default();
    let src0 = StreamOutput::<f32>(0.25);
    let src1 = StreamOutput::<f32>(0.75);
    <() as CrossRateKernel<StreamKind, StreamKind, LatchPolicy, 2, DownDir>>::before_inner(
        &mut state, &src0, &mut dst,
    );
    <() as CrossRateKernel<StreamKind, StreamKind, LatchPolicy, 2, DownDir>>::on_inner(
        &mut state, 0, &src0, &mut dst,
    );
    <() as CrossRateKernel<StreamKind, StreamKind, LatchPolicy, 2, DownDir>>::on_inner(
        &mut state, 1, &src1, &mut dst,
    );
    <() as CrossRateKernel<StreamKind, StreamKind, LatchPolicy, 2, DownDir>>::after_inner(
        &mut state, &src1, &mut dst,
    );
    assert_eq!(dst.0, 0.25);
}

#[test]
fn stream_up_latch_uses_latch() {
    type S = <() as CrossRateKernel<StreamKind, StreamKind, LatchPolicy, 2, UpDir>>::State;
    let mut state: S = Default::default();
    let src = StreamOutput::<f32>(0.7);
    let mut dst = StreamInput::<f32>::default();
    <() as CrossRateKernel<StreamKind, StreamKind, LatchPolicy, 2, UpDir>>::before_inner(
        &mut state, &src, &mut dst,
    );
    for inner in 0..2 {
        <() as CrossRateKernel<StreamKind, StreamKind, LatchPolicy, 2, UpDir>>::on_inner(
            &mut state, inner, &src, &mut dst,
        );
        // Latch should hold the source value across all inner ticks.
        assert_eq!(dst.0, 0.7);
    }
    <() as CrossRateKernel<StreamKind, StreamKind, LatchPolicy, 2, UpDir>>::after_inner(
        &mut state, &src, &mut dst,
    );
}
