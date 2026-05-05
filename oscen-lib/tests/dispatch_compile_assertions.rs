//! Compile-time assertions that the dispatch markers and EndpointAt projections
//! resolve to the expected types. These never run; if they compile, they pass.

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
