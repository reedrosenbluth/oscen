//! `(StreamKind, StreamKind, *)` impls of [`CrossRateKernel`].
//!
//! Each impl declares the per-edge `State` shape that the graph! macro reads
//! to choose a resampler-state field type. The lifecycle work — calling
//! `StreamUpsampler::upsample` and `StreamDownsampler::downsample` — is
//! performed directly by the macro's codegen against `state.kernel`, not
//! dispatched through this trait.

use crate::dispatch::{
    CrossRateKernel, DefaultPolicy, DownDir, LatchPolicy, LinearPolicy, SincIirPolicy, SincPolicy,
    StreamKind, UpDir,
};
use crate::resample::{
    IirHalfbandDown, IirHalfbandUp, LatchDown, LatchUp, LinearDown, LinearUp, SincDownFir,
    SincUpFir,
};

/// Per-edge state for stream upsampling: kernel + the `[f32; N]` precomputed
/// upsample buffer that codegen fills before the inner loop and reads on
/// each inner tick.
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
/// source-sample buffer that codegen fills inside the inner loop and consumes
/// after.
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

macro_rules! impl_stream_up {
    ($Policy:ty, $Kernel:ident, $N:literal) => {
        impl CrossRateKernel<StreamKind, StreamKind, $Policy, $N, UpDir> for () {
            type State = UpState<$Kernel<$N>, $N>;
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
