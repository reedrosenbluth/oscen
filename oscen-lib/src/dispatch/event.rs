//! `(EventKind, EventKind, _, N, Dir)` impls. Rescales `frame_offset` across
//! rate boundaries: Up multiplies by N, Down divides by N. State is a
//! unit-shaped struct because the rescaling is computed inline; no per-edge
//! memory is needed.
//!
//! Lifecycle (Up, outer -> inner):
//!   - `before_inner` clears the destination queue, then copies each source
//!     event with `frame_offset *= N`.
//!   - `on_inner` and `after_inner` are no-ops.
//!
//! Lifecycle (Down, inner -> outer):
//!   - `before_inner` and `on_inner` are no-ops; inner ticks may push events
//!     into `src` over the course of an outer tick.
//!   - `after_inner` clears the destination queue, then copies each source
//!     event with `frame_offset /= N`.
//!
//! All five `Policy` markers route to the same impl: events do not have a
//! meaningful resampler choice, but the keyword is part of the dispatch tuple
//! so it must be covered.

use crate::dispatch::{
    CrossRateKernel, DefaultPolicy, DownDir, EventKind, LatchPolicy, LinearPolicy, SincIirPolicy,
    SincPolicy, UpDir,
};
use crate::graph::{EventInput, EventOutput};

/// Per-edge state for event rescaling. No fields: the rescale is computed
/// inline per call. Carried only to satisfy the `CrossRateKernel::State` shape.
#[derive(Debug, Default)]
pub struct EventRescaleState;

// ----------------------------------------------------------------------------
// Up: outer -> inner. Multiply frame_offset by N.
// ----------------------------------------------------------------------------

macro_rules! impl_event_up {
    ($Policy:ty, $N:literal) => {
        impl CrossRateKernel<EventKind, EventKind, $Policy, $N, UpDir> for () {
            type State = EventRescaleState;
            type Src = EventOutput<f32>;
            type Dst = EventInput<f32>;

            #[inline]
            fn before_inner(_state: &mut Self::State, src: &Self::Src, dst: &mut Self::Dst) {
                dst.clear();
                for ev in src.iter() {
                    let mut ev = ev.clone();
                    ev.frame_offset = ev.frame_offset.saturating_mul($N);
                    let _ = dst.try_push(ev);
                }
            }

            #[inline]
            fn on_inner(
                _state: &mut Self::State,
                _inner: usize,
                _src: &Self::Src,
                _dst: &mut Self::Dst,
            ) {
            }

            #[inline]
            fn after_inner(_state: &mut Self::State, _src: &Self::Src, _dst: &mut Self::Dst) {}
        }
    };
}

macro_rules! impl_event_up_all_n {
    ($Policy:ty) => {
        impl_event_up!($Policy, 1);
        impl_event_up!($Policy, 2);
        impl_event_up!($Policy, 4);
        impl_event_up!($Policy, 8);
    };
}

impl_event_up_all_n!(DefaultPolicy);
impl_event_up_all_n!(LatchPolicy);
impl_event_up_all_n!(LinearPolicy);
impl_event_up_all_n!(SincPolicy);
impl_event_up_all_n!(SincIirPolicy);

// ----------------------------------------------------------------------------
// Down: inner -> outer. Divide frame_offset by N. Drain runs in after_inner so
// that any events pushed into `src` during inner ticks are seen by the rescale.
// ----------------------------------------------------------------------------

macro_rules! impl_event_down {
    ($Policy:ty, $N:literal) => {
        impl CrossRateKernel<EventKind, EventKind, $Policy, $N, DownDir> for () {
            type State = EventRescaleState;
            type Src = EventOutput<f32>;
            type Dst = EventInput<f32>;

            #[inline]
            fn before_inner(_state: &mut Self::State, _src: &Self::Src, _dst: &mut Self::Dst) {}

            #[inline]
            fn on_inner(
                _state: &mut Self::State,
                _inner: usize,
                _src: &Self::Src,
                _dst: &mut Self::Dst,
            ) {
            }

            #[inline]
            fn after_inner(_state: &mut Self::State, src: &Self::Src, dst: &mut Self::Dst) {
                dst.clear();
                for ev in src.iter() {
                    let mut ev = ev.clone();
                    ev.frame_offset /= $N;
                    let _ = dst.try_push(ev);
                }
            }
        }
    };
}

macro_rules! impl_event_down_all_n {
    ($Policy:ty) => {
        impl_event_down!($Policy, 1);
        impl_event_down!($Policy, 2);
        impl_event_down!($Policy, 4);
        impl_event_down!($Policy, 8);
    };
}

impl_event_down_all_n!(DefaultPolicy);
impl_event_down_all_n!(LatchPolicy);
impl_event_down_all_n!(LinearPolicy);
impl_event_down_all_n!(SincPolicy);
impl_event_down_all_n!(SincIirPolicy);
