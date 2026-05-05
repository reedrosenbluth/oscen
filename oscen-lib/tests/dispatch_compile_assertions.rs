//! Compile-time assertions that the dispatch markers and EndpointAt projections
//! resolve to the expected types. These never run; if they compile, they pass.

#[allow(unused_imports)]
// Used by Phase 1 tests; included here to assert presence in API surface.
use oscen::dispatch::{
    DefaultPolicy, DownDir, EndpointAt, EventArrayKind, EventKind, LatchPolicy, LinearPolicy,
    SincIirPolicy, SincPolicy, StreamKind, UpDir, ValueKind,
};

#[test]
fn marker_types_exist() {
    let _: StreamKind;
    let _: ValueKind;
    let _: EventKind;
    let _: EventArrayKind;
    let _: DefaultPolicy;
    let _: SincPolicy;
    let _: SincIirPolicy;
    let _: LinearPolicy;
    let _: LatchPolicy;
    let _: UpDir;
    let _: DownDir;
}

#[test]
fn cross_rate_kernel_trait_compiles() {
    // Defining this function proves the `CrossRateKernel` trait path is
    // reachable: rustc resolves the `CrossRateKernel` name in the where-clause
    // at type-checking time. We deliberately do NOT instantiate the function
    // — doing so would require a concrete impl, which only exists in later
    // phases of the dispatch rollout.
    #[allow(dead_code)]
    fn _assert_trait_exists<S, D, P, const N: u32, Dir>()
    where
        (): oscen::dispatch::CrossRateKernel<S, D, P, N, Dir>,
    {
    }
}

use oscen::graph::{EventInput, EventOutput, StreamInput, StreamOutput, ValueInput, ValueOutput};
use oscen::Node;

#[derive(Debug, Node)]
pub struct DispatchTestNode {
    pub stream_in: StreamInput,
    pub stream_out: StreamOutput,
    pub value_in: ValueInput,
    pub value_out: ValueOutput,
    pub event_in: EventInput,
    pub event_out: EventOutput,
}

impl DispatchTestNode {
    pub fn new() -> Self {
        Self {
            stream_in: StreamInput::default(),
            stream_out: StreamOutput::default(),
            value_in: ValueInput::default(),
            value_out: ValueOutput::default(),
            event_in: EventInput::default(),
            event_out: EventOutput::default(),
        }
    }

    fn on_event_in(&mut self, _event: &oscen::graph::EventInstance) {}
}

impl oscen::SignalProcessor for DispatchTestNode {
    fn process(&mut self) {}
}

#[test]
fn derive_emits_endpoint_at_impls() {
    use oscen::dispatch::{EndpointAt, EventKind, StreamKind, ValueKind};

    fn assert_kind<N, M, K>()
    where
        N: EndpointAt<M, Kind = K>,
    {
    }

    // Marker name format: <NodeType>__<field>__Ep
    assert_kind::<DispatchTestNode, DispatchTestNode__stream_in__Ep, StreamKind>();
    assert_kind::<DispatchTestNode, DispatchTestNode__stream_out__Ep, StreamKind>();
    assert_kind::<DispatchTestNode, DispatchTestNode__value_in__Ep, ValueKind>();
    assert_kind::<DispatchTestNode, DispatchTestNode__value_out__Ep, ValueKind>();
    assert_kind::<DispatchTestNode, DispatchTestNode__event_in__Ep, EventKind>();
    assert_kind::<DispatchTestNode, DispatchTestNode__event_out__Ep, EventKind>();
}
