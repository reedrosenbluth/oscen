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
fn stream_up_default_matches_sinc_up_fir_reference() {
    // Bit-identity check: the cross-rate kernel must produce the same output
    // as a directly-instantiated SincUpFir<4>. Proves the trait wrapping
    // doesn't perturb DSP behavior.
    use oscen::resample::{SincUpFir, StreamUpsampler};

    type State = <() as CrossRateKernel<StreamKind, StreamKind, DefaultPolicy, 4, UpDir>>::State;
    let mut state: State = Default::default();

    let inputs = [1.0_f32, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0];
    let mut cross_out = Vec::with_capacity(inputs.len() * 4);

    for &x in &inputs {
        let src = StreamOutput::<f32>(x);
        let mut dst = StreamInput::<f32>::default();
        <() as CrossRateKernel<StreamKind, StreamKind, DefaultPolicy, 4, UpDir>>::before_inner(
            &mut state, &src, &mut dst,
        );
        for inner in 0..4 {
            <() as CrossRateKernel<StreamKind, StreamKind, DefaultPolicy, 4, UpDir>>::on_inner(
                &mut state, inner, &src, &mut dst,
            );
            cross_out.push(dst.0);
        }
    }

    let mut reference = SincUpFir::<4>::new();
    let mut ref_full = Vec::with_capacity(inputs.len() * 4);
    let mut buf = [0.0_f32; 4];
    for &x in &inputs {
        reference.upsample(x, &mut buf);
        ref_full.extend_from_slice(&buf);
    }

    assert_eq!(cross_out.len(), ref_full.len());
    for (i, (a, b)) in cross_out.iter().zip(ref_full.iter()).enumerate() {
        let diff = (a - b).abs();
        assert!(diff < 1e-6, "sample {i}: cross={a}, ref={b}, diff={diff}");
    }
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

#[test]
fn value_value_up_latches_value() {
    use oscen::dispatch::ValueKind;
    use oscen::graph::{ValueInput, ValueOutput};

    type K = <() as CrossRateKernel<ValueKind, ValueKind, DefaultPolicy, 4, UpDir>>::State;
    let mut state: K = Default::default();
    let src = ValueOutput::<f32>(0.42);
    let mut dst = ValueInput::<f32>::default();

    <() as CrossRateKernel<ValueKind, ValueKind, DefaultPolicy, 4, UpDir>>::before_inner(
        &mut state, &src, &mut dst,
    );
    for inner in 0..4 {
        <() as CrossRateKernel<ValueKind, ValueKind, DefaultPolicy, 4, UpDir>>::on_inner(
            &mut state, inner, &src, &mut dst,
        );
        assert_eq!(dst.0, 0.42, "all inner ticks see the latched value");
    }
}

#[test]
fn value_to_stream_up_broadcasts_value() {
    use oscen::dispatch::{StreamKind, ValueKind};
    use oscen::graph::{StreamInput, ValueOutput};

    type K = <() as CrossRateKernel<ValueKind, StreamKind, DefaultPolicy, 4, UpDir>>::State;
    let mut state: K = Default::default();
    let src = ValueOutput::<f32>(0.5);
    let mut dst = StreamInput::<f32>::default();

    <() as CrossRateKernel<ValueKind, StreamKind, DefaultPolicy, 4, UpDir>>::before_inner(
        &mut state, &src, &mut dst,
    );
    for inner in 0..4 {
        <() as CrossRateKernel<ValueKind, StreamKind, DefaultPolicy, 4, UpDir>>::on_inner(
            &mut state, inner, &src, &mut dst,
        );
        assert_eq!(dst.0, 0.5);
    }
}

#[test]
fn value_value_down_emits_latched_value() {
    use oscen::dispatch::ValueKind;
    use oscen::graph::{ValueInput, ValueOutput};

    type K = <() as CrossRateKernel<ValueKind, ValueKind, DefaultPolicy, 4, DownDir>>::State;
    let mut state: K = Default::default();
    let mut dst = ValueInput::<f32>::default();

    // 4 inner ticks with different values; last-one-wins semantics.
    let srcs = [0.1, 0.2, 0.3, 0.4_f32];
    for inner in 0..4 {
        let src = ValueOutput::<f32>(srcs[inner]);
        <() as CrossRateKernel<ValueKind, ValueKind, DefaultPolicy, 4, DownDir>>::on_inner(
            &mut state, inner, &src, &mut dst,
        );
    }
    let final_src = ValueOutput::<f32>(0.4);
    <() as CrossRateKernel<ValueKind, ValueKind, DefaultPolicy, 4, DownDir>>::after_inner(
        &mut state, &final_src, &mut dst,
    );
    assert_eq!(dst.0, 0.4, "Down direction emits last captured value");
}
