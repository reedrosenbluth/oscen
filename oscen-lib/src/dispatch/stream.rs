//! `(StreamKind, StreamKind, *)` impls of [`CrossRateKernel`].
//!
//! Each impl declares the per-edge `State` shape that the graph! macro reads
//! to choose a resampler-state field type. The lifecycle work — calling
//! `StreamUpsampler::upsample` and `StreamDownsampler::downsample` — is
//! performed directly by the macro's codegen against `state.kernel`, not
//! dispatched through this trait.
//!
//! Every kernel family is genericized over [`AudioFrame`], so all impls here
//! are blanket impls over `F: AudioFrame` (with `F = f32` as the mono case).

use crate::dispatch::{
    CrossRateKernel, DefaultPolicy, DownDir, LatchPolicy, LinearPolicy, SincIirPolicy, SincPolicy,
    StreamKind, UpDir,
};
use crate::frame::AudioFrame;
use crate::resample::{
    IirHalfbandDown, IirHalfbandUp, LatchDown, LatchUp, LinearDown, LinearUp, SincDownFir,
    SincUpFir,
};

/// Per-edge state for stream upsampling: kernel + the `[F; N]` precomputed
/// upsample buffer that codegen fills before the inner loop and reads on
/// each inner tick.
#[derive(Debug)]
pub struct UpState<K, const N: usize, F = f32> {
    pub kernel: K,
    pub buffer: [F; N],
}

impl<K: Default, const N: usize, F: Default> Default for UpState<K, N, F> {
    fn default() -> Self {
        Self {
            kernel: K::default(),
            buffer: core::array::from_fn(|_| F::default()),
        }
    }
}

/// Per-edge state for stream downsampling: kernel + the `[F; N]` captured
/// source-sample buffer that codegen fills inside the inner loop and consumes
/// after.
#[derive(Debug)]
pub struct DownState<K, const N: usize, F = f32> {
    pub kernel: K,
    pub buffer: [F; N],
}

impl<K: Default, const N: usize, F: Default> Default for DownState<K, N, F> {
    fn default() -> Self {
        Self {
            kernel: K::default(),
            buffer: core::array::from_fn(|_| F::default()),
        }
    }
}

macro_rules! impl_stream_up_framed {
    ($Policy:ty, $Kernel:ident, $N:literal) => {
        impl<F: AudioFrame> CrossRateKernel<StreamKind, StreamKind, $Policy, $N, UpDir, F> for () {
            type State = UpState<$Kernel<$N, F>, $N, F>;
        }
    };
}

macro_rules! impl_stream_up_framed_all_n {
    ($Policy:ty, $Kernel:ident) => {
        impl_stream_up_framed!($Policy, $Kernel, 1);
        impl_stream_up_framed!($Policy, $Kernel, 2);
        impl_stream_up_framed!($Policy, $Kernel, 4);
        impl_stream_up_framed!($Policy, $Kernel, 8);
    };
}

macro_rules! impl_stream_down_framed {
    ($Policy:ty, $Kernel:ident, $N:literal) => {
        impl<F: AudioFrame> CrossRateKernel<StreamKind, StreamKind, $Policy, $N, DownDir, F>
            for ()
        {
            type State = DownState<$Kernel<$N, F>, $N, F>;
        }
    };
}

macro_rules! impl_stream_down_framed_all_n {
    ($Policy:ty, $Kernel:ident) => {
        impl_stream_down_framed!($Policy, $Kernel, 1);
        impl_stream_down_framed!($Policy, $Kernel, 2);
        impl_stream_down_framed!($Policy, $Kernel, 4);
        impl_stream_down_framed!($Policy, $Kernel, 8);
    };
}

impl_stream_up_framed_all_n!(DefaultPolicy, SincUpFir);
impl_stream_up_framed_all_n!(SincPolicy, SincUpFir);
impl_stream_up_framed_all_n!(SincIirPolicy, IirHalfbandUp);
impl_stream_up_framed_all_n!(LinearPolicy, LinearUp);
impl_stream_up_framed_all_n!(LatchPolicy, LatchUp);

impl_stream_down_framed_all_n!(DefaultPolicy, SincDownFir);
impl_stream_down_framed_all_n!(SincPolicy, SincDownFir);
impl_stream_down_framed_all_n!(SincIirPolicy, IirHalfbandDown);
impl_stream_down_framed_all_n!(LinearPolicy, LinearDown);
impl_stream_down_framed_all_n!(LatchPolicy, LatchDown);
