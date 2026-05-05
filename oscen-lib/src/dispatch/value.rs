//! `(ValueKind, *)` impls of [`CrossRateKernel`].
//!
//! Value cross-rate edges always latch (under any `[policy]` keyword the user
//! might write — values don't have a meaningful resampler). The Up direction
//! captures the source value once per outer tick and replays it across all `N`
//! inner ticks; the Down direction captures inner ticks (last-one-wins) and
//! emits the latched value at the outer-tick boundary.

use crate::dispatch::{
    CrossRateKernel, DefaultPolicy, DownDir, LatchPolicy, LinearPolicy, SincIirPolicy, SincPolicy,
    UpDir, ValueKind,
};
use crate::graph::{ValueInput, ValueOutput};

/// Per-edge state for value latch: stores the latched `f32`.
///
/// Field is `pub` for ergonomic access from macro-generated impls in this
/// module; the struct is only constructed by `CrossRateKernel` impls.
#[derive(Debug, Default)]
pub struct ValueLatchState {
    pub held: f32,
}

// ----------------------------------------------------------------------------
// (ValueKind, ValueKind)
// ----------------------------------------------------------------------------

// Up: outer -> inner. Capture in before_inner, replay in on_inner.
macro_rules! impl_value_up {
    ($Policy:ty, $N:literal) => {
        impl CrossRateKernel<ValueKind, ValueKind, $Policy, $N, UpDir> for () {
            type State = ValueLatchState;
            type Src = ValueOutput<f32>;
            type Dst = ValueInput<f32>;

            #[inline]
            fn before_inner(state: &mut Self::State, src: &Self::Src, _dst: &mut Self::Dst) {
                state.held = src.0;
            }

            #[inline]
            fn on_inner(
                state: &mut Self::State,
                _inner: usize,
                _src: &Self::Src,
                dst: &mut Self::Dst,
            ) {
                dst.0 = state.held;
            }

            #[inline]
            fn after_inner(_state: &mut Self::State, _src: &Self::Src, _dst: &mut Self::Dst) {}
        }
    };
}

macro_rules! impl_value_up_all_n {
    ($Policy:ty) => {
        impl_value_up!($Policy, 1);
        impl_value_up!($Policy, 2);
        impl_value_up!($Policy, 4);
        impl_value_up!($Policy, 8);
    };
}

impl_value_up_all_n!(DefaultPolicy);
impl_value_up_all_n!(LatchPolicy);
impl_value_up_all_n!(LinearPolicy);
impl_value_up_all_n!(SincPolicy);
impl_value_up_all_n!(SincIirPolicy);

// Down: inner -> outer. Capture in on_inner (last-one-wins), emit in
// after_inner.
macro_rules! impl_value_down {
    ($Policy:ty, $N:literal) => {
        impl CrossRateKernel<ValueKind, ValueKind, $Policy, $N, DownDir> for () {
            type State = ValueLatchState;
            type Src = ValueOutput<f32>;
            type Dst = ValueInput<f32>;

            #[inline]
            fn before_inner(_state: &mut Self::State, _src: &Self::Src, _dst: &mut Self::Dst) {}

            #[inline]
            fn on_inner(
                state: &mut Self::State,
                _inner: usize,
                src: &Self::Src,
                _dst: &mut Self::Dst,
            ) {
                state.held = src.0;
            }

            #[inline]
            fn after_inner(state: &mut Self::State, _src: &Self::Src, dst: &mut Self::Dst) {
                dst.0 = state.held;
            }
        }
    };
}

macro_rules! impl_value_down_all_n {
    ($Policy:ty) => {
        impl_value_down!($Policy, 1);
        impl_value_down!($Policy, 2);
        impl_value_down!($Policy, 4);
        impl_value_down!($Policy, 8);
    };
}

impl_value_down_all_n!(DefaultPolicy);
impl_value_down_all_n!(LatchPolicy);
impl_value_down_all_n!(LinearPolicy);
impl_value_down_all_n!(SincPolicy);
impl_value_down_all_n!(SincIirPolicy);
