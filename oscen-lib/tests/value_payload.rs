//! Stage-1 typed value endpoints:
//!
//! - `ValuePayload` is the opt-in marker for typed value payloads (enums,
//!   bools, small `Copy` structs) — plain data that latches between nodes
//!   without the f32 parameter machinery (registry, ramping, resampling);
//! - the `ReadValueEndpoint` / `ConnectEndpoints` blanket impls cover any
//!   `ValuePayload` (including `f32`) while `ValueRampState` keeps its
//!   dedicated impls;
//! - a `#[derive(Node)]` type with a non-f32 value field derives cleanly
//!   and its endpoint manifest carries the field's literal type tokens
//!   (`ty = …`), mirroring what frame-typed stream endpoints already do.
//!
//! Graph-level typed value connections are stage 2; these tests exercise
//! the runtime traits and the derive manifest directly.
#![feature(inherent_associated_types)]

use oscen::graph::{ConnectEndpoints, ReadValueEndpoint, ValuePayload, ValueRampState};
use oscen::Node;

// ---------------------------------------------------------------------------
// A user-defined typed payload: plain Copy data opting in via the marker.
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Default, Debug, PartialEq)]
pub enum FilterMode {
    #[default]
    Lowpass,
    Highpass,
    Bandpass,
}

impl ValuePayload for FilterMode {}

#[test]
fn read_value_blanket_covers_custom_payloads_and_f32() {
    // Custom payload: the blanket impl reads the field back as itself.
    let mode = FilterMode::Highpass;
    assert_eq!(ReadValueEndpoint::read_value(&mode), FilterMode::Highpass);

    // f32 goes through the same blanket impl (Value = f32), so existing
    // generated call sites keep inferring f32.
    let freq = 440.0f32;
    assert_eq!(ReadValueEndpoint::read_value(&freq), 440.0);
}

#[test]
fn read_value_ramp_state_still_reads_current_f32() {
    // `ValueRampState` keeps its dedicated impl: it reads as the ramp's
    // current f32, not as the state struct.
    let ramp = ValueRampState::new(220.0);
    assert_eq!(ReadValueEndpoint::read_value(&ramp), 220.0);
}

#[test]
fn connect_blanket_copies_custom_payloads_and_f32() {
    // Same-type typed value edge: per-frame copy (a latch — the last
    // written value holds).
    let src = FilterMode::Bandpass;
    let mut dst = FilterMode::default();
    <() as ConnectEndpoints<_, _>>::connect(&src, &mut dst);
    assert_eq!(dst, FilterMode::Bandpass);

    // f32 edges route through the same blanket impl.
    let fsrc = 0.5f32;
    let mut fdst = 0.0f32;
    <() as ConnectEndpoints<_, _>>::connect(&fsrc, &mut fdst);
    assert_eq!(fdst, 0.5);

    // Other built-in payloads (bool here) get the same treatment.
    let bsrc = true;
    let mut bdst = false;
    <() as ConnectEndpoints<_, _>>::connect(&bsrc, &mut bdst);
    assert!(bdst);
}

// ---------------------------------------------------------------------------
// Derive side: a non-f32 value field derives cleanly and the manifest
// carries `ty = <literal field type tokens>`. Plain f32 value fields stay
// unannotated; `ValueRampState` fields stay `ramped`.
// ---------------------------------------------------------------------------

#[derive(Debug, Node)]
pub struct ModeFilter {
    #[input(value)]
    pub mode: FilterMode,
    #[input(value)]
    pub cutoff: f32,
    #[input(value)]
    pub resonance: ValueRampState,
    #[output(value)]
    pub active_mode: FilterMode,
    #[output(stream)]
    pub out: f32,
}

macro_rules! collect_manifest {
    (
        probe
        node_type $ty:ident
        inputs [ $($in_name:ident : $in_kind:ident $(( $($in_meta:tt)* ))?),* ]
        outputs [ $($out_name:ident : $out_kind:ident $(( $($out_meta:tt)* ))?),* ]
    ) => {
        const MANIFEST_TYPE: &str = stringify!($ty);
        const MANIFEST_INPUTS: &[(&str, &str, &str)] =
            &[ $( (
                stringify!($in_name),
                stringify!($in_kind),
                stringify!($($($in_meta)*)?),
            ) ),* ];
        const MANIFEST_OUTPUTS: &[(&str, &str, &str)] =
            &[ $( (
                stringify!($out_name),
                stringify!($out_kind),
                stringify!($($($out_meta)*)?),
            ) ),* ];
    };
}

__oscen_endpoints_ModeFilter!(collect_manifest => (probe));

#[test]
fn derive_manifest_carries_typed_value_payload_type() {
    assert_eq!(MANIFEST_TYPE, "ModeFilter");
    assert_eq!(
        MANIFEST_INPUTS,
        &[
            ("mode", "value", "ty = FilterMode"),
            ("cutoff", "value", ""),
            ("resonance", "value", "ramped"),
        ]
    );
    assert_eq!(
        MANIFEST_OUTPUTS,
        &[
            ("active_mode", "value", "ty = FilterMode"),
            ("out", "stream", ""),
        ]
    );
}
