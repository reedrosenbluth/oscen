//! `(StreamKind, StreamKind, *)` impls of [`CrossRateKernel`].
//!
//! Each impl wraps a concrete kernel from [`crate::resample`] in a per-edge
//! state struct ([`UpState`] / [`DownState`]) that owns both the kernel and a
//! `[f32; N]` working buffer.
//!
//! Lifecycle (Up):
//!   - `before_inner` calls [`StreamUpsampler::upsample`] once with the source
//!     sample, filling `state.buffer` with `N` destination samples.
//!   - `on_inner` writes `state.buffer[inner]` to the destination on each of
//!     the `N` inner ticks.
//!   - `after_inner` is a no-op.
//!
//! Lifecycle (Down):
//!   - `before_inner` is a no-op.
//!   - `on_inner` captures the current source sample into `state.buffer[inner]`
//!     on each of the `N` inner ticks.
//!   - `after_inner` calls [`StreamDownsampler::downsample`] on the captured
//!     buffer and writes the single destination sample.

use crate::dispatch::{
    CrossRateKernel, DefaultPolicy, DownDir, LatchPolicy, LinearPolicy, SincIirPolicy, SincPolicy,
    StreamKind, UpDir,
};
use crate::graph::{StreamInput, StreamOutput};
use crate::resample::{
    IirHalfbandDown, IirHalfbandUp, LatchDown, LatchUp, LinearDown, LinearUp, SincDownFir,
    SincUpFir, StreamDownsampler, StreamUpsampler,
};

/// Per-edge state for stream upsampling: kernel + the `[f32; N]` precomputed
/// upsample buffer that `before_inner` fills and `on_inner` reads from.
///
/// Fields are `pub` for ergonomic access from macro-generated impls in this
/// module; the struct is only constructed by `CrossRateKernel` impls.
#[derive(Debug)]
pub struct UpState<K, const N: usize> {
    pub kernel: K,
    pub buffer: [f32; N],
}

impl<K: Default, const N: usize> Default for UpState<K, N> {
    fn default() -> Self {
        Self {
            kernel: K::default(),
            buffer: [0.0; N],
        }
    }
}

/// Per-edge state for stream downsampling: kernel + the `[f32; N]` captured
/// source-sample buffer that `on_inner` fills and `after_inner` consumes.
///
/// Fields are `pub` for ergonomic access from macro-generated impls in this
/// module; the struct is only constructed by `CrossRateKernel` impls.
#[derive(Debug)]
pub struct DownState<K, const N: usize> {
    pub kernel: K,
    pub buffer: [f32; N],
}

impl<K: Default, const N: usize> Default for DownState<K, N> {
    fn default() -> Self {
        Self {
            kernel: K::default(),
            buffer: [0.0; N],
        }
    }
}

// ----------------------------------------------------------------------------
// Macros to emit the per-(Policy, N) impls for each direction.
//
// Coherence-wise, each tuple is a unique impl. We expand one impl per (Policy,
// N, Dir) for N ∈ {1, 2, 4, 8} — the const factors supported by the underlying
// kernels.
// ----------------------------------------------------------------------------

macro_rules! impl_stream_up {
    ($Policy:ty, $Kernel:ident, $N:literal) => {
        impl CrossRateKernel<StreamKind, StreamKind, $Policy, $N, UpDir> for () {
            type State = UpState<$Kernel<$N>, $N>;
            type Src = StreamOutput<f32>;
            type Dst = StreamInput<f32>;

            #[inline]
            fn before_inner(state: &mut Self::State, src: &Self::Src, _dst: &mut Self::Dst) {
                state.kernel.upsample(src.0, &mut state.buffer);
            }

            #[inline]
            fn on_inner(
                state: &mut Self::State,
                inner: usize,
                _src: &Self::Src,
                dst: &mut Self::Dst,
            ) {
                dst.0 = state.buffer[inner];
            }

            #[inline]
            fn after_inner(_state: &mut Self::State, _src: &Self::Src, _dst: &mut Self::Dst) {}
        }
    };
}

macro_rules! impl_stream_up_all_n {
    ($Policy:ty, $Kernel:ident) => {
        impl_stream_up!($Policy, $Kernel, 1);
        impl_stream_up!($Policy, $Kernel, 2);
        impl_stream_up!($Policy, $Kernel, 4);
        impl_stream_up!($Policy, $Kernel, 8);
    };
}

macro_rules! impl_stream_down {
    ($Policy:ty, $Kernel:ident, $N:literal) => {
        impl CrossRateKernel<StreamKind, StreamKind, $Policy, $N, DownDir> for () {
            type State = DownState<$Kernel<$N>, $N>;
            type Src = StreamOutput<f32>;
            type Dst = StreamInput<f32>;

            #[inline]
            fn before_inner(_state: &mut Self::State, _src: &Self::Src, _dst: &mut Self::Dst) {}

            #[inline]
            fn on_inner(
                state: &mut Self::State,
                inner: usize,
                src: &Self::Src,
                _dst: &mut Self::Dst,
            ) {
                state.buffer[inner] = src.0;
            }

            #[inline]
            fn after_inner(state: &mut Self::State, _src: &Self::Src, dst: &mut Self::Dst) {
                dst.0 = state.kernel.downsample(&state.buffer);
            }
        }
    };
}

macro_rules! impl_stream_down_all_n {
    ($Policy:ty, $Kernel:ident) => {
        impl_stream_down!($Policy, $Kernel, 1);
        impl_stream_down!($Policy, $Kernel, 2);
        impl_stream_down!($Policy, $Kernel, 4);
        impl_stream_down!($Policy, $Kernel, 8);
    };
}

// `Default` and `Sinc` both pick the FIR sinc kernel; downstream the macro
// distinguishes them at the `ConnectionPolicy` level even if the kernel is
// the same today.
impl_stream_up_all_n!(DefaultPolicy, SincUpFir);
impl_stream_up_all_n!(SincPolicy, SincUpFir);
impl_stream_up_all_n!(SincIirPolicy, IirHalfbandUp);
impl_stream_up_all_n!(LinearPolicy, LinearUp);
impl_stream_up_all_n!(LatchPolicy, LatchUp);

impl_stream_down_all_n!(DefaultPolicy, SincDownFir);
impl_stream_down_all_n!(SincPolicy, SincDownFir);
impl_stream_down_all_n!(SincIirPolicy, IirHalfbandDown);
impl_stream_down_all_n!(LinearPolicy, LinearDown);
impl_stream_down_all_n!(LatchPolicy, LatchDown);
