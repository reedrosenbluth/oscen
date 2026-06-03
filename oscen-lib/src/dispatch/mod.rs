//! Type-level dispatch for cross-rate graph edges.
//!
//! Replaces the macro's runtime kind-inference pass with a coherence-driven
//! dispatch table. `EndpointAt` exposes each node endpoint's kind to the type
//! system; `CrossRateKernel` impls map `(SrcKind, DstKind, Policy)` tuples to
//! concrete resampler/latch/event-rescale state.

pub mod event;
pub mod stream;
pub mod value;

/// Endpoint-kind markers. Emitted by `#[derive(Node)]` as the `Kind` associated
/// type of each endpoint's `EndpointAt` impl.
#[derive(Debug, Clone, Copy, Default)]
pub struct StreamKind;

#[derive(Debug, Clone, Copy, Default)]
pub struct ValueKind;

#[derive(Debug, Clone, Copy, Default)]
pub struct EventKind;

/// `[EventOutput; N]` voice-allocator-style endpoints. Recognized by the type
/// system but not dispatched cross-rate (no `CrossRateKernel` impl).
#[derive(Debug, Clone, Copy, Default)]
pub struct EventArrayKind;

/// Policy markers, one per `ConnectionPolicy` variant in `oscen-macros`.
#[derive(Debug, Clone, Copy, Default)]
pub struct DefaultPolicy;
#[derive(Debug, Clone, Copy, Default)]
pub struct SincPolicy;
#[derive(Debug, Clone, Copy, Default)]
pub struct SincIirPolicy;
#[derive(Debug, Clone, Copy, Default)]
pub struct LinearPolicy;
#[derive(Debug, Clone, Copy, Default)]
pub struct LatchPolicy;

/// Cross-rate direction markers.
#[derive(Debug, Clone, Copy, Default)]
pub struct UpDir;

#[derive(Debug, Clone, Copy, Default)]
pub struct DownDir;

/// Per-endpoint type-system query. The graph macro projects `<Node as
/// EndpointAt<EndpointMarker>>::Kind` to determine an endpoint's kind without
/// querying trait impls at expansion time.
pub trait EndpointAt<Marker> {
    type Kind;
    type Frame: crate::frame::AudioFrame;
}

/// Cross-rate edge state-shape table. Coherence picks an impl from the
/// `(SrcKind, DstKind, Policy)` tuple plus the const factor `N` and direction
/// `Dir`. The trait is a type-level state-shape registry consumed by:
///
/// 1. The `graph!` macro's `::State` projection — chooses the field type for
///    each cross-rate edge's resampler state on stream/stream edges.
/// 2. A const-time trait-bound assertion emitted by codegen per cross-rate
///    edge — drives `on_unimplemented` to surface unsupported kind tuples
///    with a span at the user's connection token.
///
/// Lifecycle ordering (warmup, per-tick, finalize) is owned by the macro's
/// codegen; per-edge work is performed by direct calls to the concrete
/// resampler traits (`StreamUpsampler` / `StreamDownsampler`) on
/// `state.kernel`. There is no `before_inner` / `on_inner` / `after_inner`
/// dispatch through this trait.
#[diagnostic::on_unimplemented(
    message = "no cross-rate kernel for {SrcKind} -> {DstKind} with policy {Policy}",
    note = "valid kind pairs are: (StreamKind, StreamKind), (ValueKind, ValueKind), (ValueKind, StreamKind), (EventKind, EventKind)",
    label = "edge has no resampler"
)]
pub trait CrossRateKernel<SrcKind, DstKind, Policy, const N: u32, Dir> {
    type State: Default + Send;
}

#[doc(hidden)]
pub mod __private_assert {
    pub trait IsStream {}
    impl IsStream for super::StreamKind {}
}
