//! Type-level dispatch for cross-rate graph edges.
//!
//! Replaces the macro's runtime kind-inference pass with a coherence-driven
//! dispatch table. `EndpointAt` exposes each node endpoint's kind to the type
//! system; `CrossRateKernel` impls map `(SrcKind, DstKind, Policy)` tuples to
//! concrete resampler/latch/event-rescale state.

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
}

/// Cross-rate edge kernel. Coherence picks an impl from the
/// `(SrcKind, DstKind, Policy)` tuple plus the const factor `N` and direction
/// `Dir`. The graph macro emits a `<() as CrossRateKernel<...>>::State` field
/// per cross-rate edge and three lifecycle method calls per outer tick.
///
/// Each impl is responsible for the entire per-edge work for its kind tuple.
/// Lifecycle phases that don't apply to a given kind are no-op'd and inlined
/// out by the optimizer.
///
/// `Src` and `Dst` are associated types so each `(SrcKind, DstKind, Policy, N,
/// Dir)` tuple commits to a concrete pair (e.g. `StreamOutput<f32>` ->
/// `StreamInput<f32>` for stream edges). This lets impls perform concrete
/// reads/writes without having to plumb `?Sized` generic parameters through
/// every method — Rust forbids impls from adding `where`-clauses to
/// trait-method generics.
#[diagnostic::on_unimplemented(
    message = "no cross-rate kernel for {SrcKind} -> {DstKind} with policy {Policy}",
    note = "valid kind pairs are: (StreamKind, StreamKind), (ValueKind, ValueKind), (ValueKind, StreamKind), (EventKind, EventKind)",
    label = "edge has no resampler"
)]
pub trait CrossRateKernel<SrcKind, DstKind, Policy, const N: u32, Dir> {
    type State: Default + Send;
    type Src: ?Sized;
    type Dst: ?Sized;

    fn before_inner(state: &mut Self::State, src: &Self::Src, dst: &mut Self::Dst);

    fn on_inner(state: &mut Self::State, inner: usize, src: &Self::Src, dst: &mut Self::Dst);

    fn after_inner(state: &mut Self::State, src: &Self::Src, dst: &mut Self::Dst);
}
