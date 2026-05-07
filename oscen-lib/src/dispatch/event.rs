//! `(EventKind, EventKind, _, N, Dir)` impls of [`CrossRateKernel`].
//!
//! Type-table only. Each impl declares the per-edge `State` shape so that
//! Phase 3's codegen const-assertion can verify `(Event, Event)` cross-rate
//! kind tuples are supported. Codegen's kind-gate keeps event edges off the
//! `::State` projection path at runtime, so the `State` here is `()` —
//! never queried, never read.
//!
//! Event cross-rate edges are handled by dedicated event drains in
//! `oscen-macros`'s codegen, which rescale `EventInstance::frame_offset`
//! per the rate factor.

use crate::dispatch::{
    CrossRateKernel, DefaultPolicy, DownDir, EventKind, LatchPolicy, LinearPolicy, SincIirPolicy,
    SincPolicy, UpDir,
};

macro_rules! impl_event_table {
    ($Policy:ty, $N:literal, $Dir:ty) => {
        impl CrossRateKernel<EventKind, EventKind, $Policy, $N, $Dir> for () {
            type State = ();
        }
    };
}

macro_rules! impl_event_table_all_n {
    ($Policy:ty, $Dir:ty) => {
        impl_event_table!($Policy, 1, $Dir);
        impl_event_table!($Policy, 2, $Dir);
        impl_event_table!($Policy, 4, $Dir);
        impl_event_table!($Policy, 8, $Dir);
    };
}

macro_rules! impl_event_table_all_policies {
    ($Dir:ty) => {
        impl_event_table_all_n!(DefaultPolicy, $Dir);
        impl_event_table_all_n!(LatchPolicy, $Dir);
        impl_event_table_all_n!(LinearPolicy, $Dir);
        impl_event_table_all_n!(SincPolicy, $Dir);
        impl_event_table_all_n!(SincIirPolicy, $Dir);
    };
}

impl_event_table_all_policies!(UpDir);
impl_event_table_all_policies!(DownDir);
