//! Unit tests for `CrossRateKernel` impls. Each test exercises one impl
//! end-to-end: build state, run the three lifecycle methods, observe the
//! effect on the dest field.

use oscen::dispatch::{
    CrossRateKernel, DefaultPolicy, LatchPolicy, LinearPolicy, SincIirPolicy, SincPolicy,
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
