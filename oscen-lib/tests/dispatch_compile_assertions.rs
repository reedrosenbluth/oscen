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
