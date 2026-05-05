//! Type-level dispatch for cross-rate graph edges.
//!
//! Replaces the macro's runtime kind-inference pass with a coherence-driven
//! dispatch table. `EndpointAt` exposes each node endpoint's kind to the type
//! system; `CrossRateKernel` impls map `(SrcKind, DstKind, Policy)` tuples to
//! concrete resampler/latch/event-rescale state.

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
}
