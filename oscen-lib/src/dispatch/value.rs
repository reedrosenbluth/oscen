//! `(ValueKind, *)` impls of [`CrossRateKernel`].
//!
//! Type-table only. Each impl declares the per-edge `State` shape so that
//! Phase 3's codegen const-assertion can verify `(Value, Value)` and
//! `(Value, Stream)` cross-rate kind tuples are supported. Codegen's
//! kind-gate keeps these edges off the `::State` projection path at
//! runtime, so the `State` here is `()` — never queried, never read.
//!
//! Value cross-rate edges are handled by the `kernel_up_type` /
//! `kernel_down_type` concrete-kernel fallback in `oscen-macros`'s codegen
//! (`LatchUp`/`LatchDown`).

use crate::dispatch::{
    CrossRateKernel, DefaultPolicy, DownDir, LatchPolicy, LinearPolicy, SincIirPolicy, SincPolicy,
    StreamKind, UpDir, ValueKind,
};

macro_rules! impl_value_table {
    ($SrcKind:ty, $DstKind:ty, $Policy:ty, $N:literal, $Dir:ty) => {
        impl CrossRateKernel<$SrcKind, $DstKind, $Policy, $N, $Dir> for () {
            type State = ();
        }
    };
}

macro_rules! impl_value_table_all_n {
    ($SrcKind:ty, $DstKind:ty, $Policy:ty, $Dir:ty) => {
        impl_value_table!($SrcKind, $DstKind, $Policy, 1, $Dir);
        impl_value_table!($SrcKind, $DstKind, $Policy, 2, $Dir);
        impl_value_table!($SrcKind, $DstKind, $Policy, 4, $Dir);
        impl_value_table!($SrcKind, $DstKind, $Policy, 8, $Dir);
    };
}

macro_rules! impl_value_table_all_policies {
    ($SrcKind:ty, $DstKind:ty, $Dir:ty) => {
        impl_value_table_all_n!($SrcKind, $DstKind, DefaultPolicy, $Dir);
        impl_value_table_all_n!($SrcKind, $DstKind, LatchPolicy, $Dir);
        impl_value_table_all_n!($SrcKind, $DstKind, LinearPolicy, $Dir);
        impl_value_table_all_n!($SrcKind, $DstKind, SincPolicy, $Dir);
        impl_value_table_all_n!($SrcKind, $DstKind, SincIirPolicy, $Dir);
    };
}

// (Value, Value)
impl_value_table_all_policies!(ValueKind, ValueKind, UpDir);
impl_value_table_all_policies!(ValueKind, ValueKind, DownDir);

// (Value, Stream)
impl_value_table_all_policies!(ValueKind, StreamKind, UpDir);
impl_value_table_all_policies!(ValueKind, StreamKind, DownDir);
