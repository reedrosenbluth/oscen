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

#[derive(Debug, Node)]
pub struct DispatchTestVoiceAllocator {
    #[input(event)]
    pub note_on: EventInput,
    #[output(event)]
    pub voices: [EventOutput; 4],
}

impl DispatchTestVoiceAllocator {
    pub fn new() -> Self {
        Self {
            note_on: EventInput::default(),
            voices: std::array::from_fn(|_| EventOutput::default()),
        }
    }

    fn on_note_on(&mut self, _event: &oscen::graph::EventInstance) {}
}

impl oscen::SignalProcessor for DispatchTestVoiceAllocator {
    fn process(&mut self) {}
}

#[test]
fn derive_maps_event_arrays_to_event_array_kind() {
    use oscen::dispatch::{EndpointAt, EventArrayKind, EventKind};

    fn assert_kind<N, M, K>()
    where
        N: EndpointAt<M, Kind = K>,
    {
    }

    assert_kind::<DispatchTestVoiceAllocator, DispatchTestVoiceAllocator__note_on__Ep, EventKind>();
    assert_kind::<DispatchTestVoiceAllocator, DispatchTestVoiceAllocator__voices__Ep, EventArrayKind>(
    );
}

#[test]
fn cross_rate_kernel_state_types_match_table() {
    // Verify each (StreamKind, StreamKind, Policy, N, Dir) tuple resolves to
    // the exact UpState/DownState wrapper around the expected resampler kernel.
    // Uses TypeId equality — any of the wrappers diverging from the dispatch
    // table (e.g. the macro picking the wrong kernel) fails this test.
    use oscen::dispatch::*;
    use oscen::resample::*;

    fn assert_state<S, D, P, const N: u32, Dir, Expected>()
    where
        (): CrossRateKernel<S, D, P, N, Dir>,
        <() as CrossRateKernel<S, D, P, N, Dir>>::State: ::core::any::Any,
        Expected: ::core::any::Any,
    {
        let s_id = ::core::any::TypeId::of::<<() as CrossRateKernel<S, D, P, N, Dir>>::State>();
        let e_id = ::core::any::TypeId::of::<Expected>();
        assert_eq!(s_id, e_id);
    }

    // Up direction
    assert_state::<
        StreamKind,
        StreamKind,
        DefaultPolicy,
        4,
        UpDir,
        oscen::dispatch::stream::UpState<SincUpFir<4>, 4>,
    >();
    assert_state::<
        StreamKind,
        StreamKind,
        SincPolicy,
        2,
        UpDir,
        oscen::dispatch::stream::UpState<SincUpFir<2>, 2>,
    >();
    assert_state::<
        StreamKind,
        StreamKind,
        SincIirPolicy,
        8,
        UpDir,
        oscen::dispatch::stream::UpState<IirHalfbandUp<8>, 8>,
    >();
    assert_state::<
        StreamKind,
        StreamKind,
        LinearPolicy,
        2,
        UpDir,
        oscen::dispatch::stream::UpState<LinearUp<2>, 2>,
    >();
    assert_state::<
        StreamKind,
        StreamKind,
        LatchPolicy,
        4,
        UpDir,
        oscen::dispatch::stream::UpState<LatchUp<4>, 4>,
    >();

    // Down direction
    assert_state::<
        StreamKind,
        StreamKind,
        DefaultPolicy,
        2,
        DownDir,
        oscen::dispatch::stream::DownState<SincDownFir<2>, 2>,
    >();
    assert_state::<
        StreamKind,
        StreamKind,
        SincIirPolicy,
        4,
        DownDir,
        oscen::dispatch::stream::DownState<IirHalfbandDown<4>, 4>,
    >();
    assert_state::<
        StreamKind,
        StreamKind,
        LinearPolicy,
        4,
        DownDir,
        oscen::dispatch::stream::DownState<LinearDown<4>, 4>,
    >();
    assert_state::<
        StreamKind,
        StreamKind,
        LatchPolicy,
        8,
        DownDir,
        oscen::dispatch::stream::DownState<LatchDown<8>, 8>,
    >();
}
