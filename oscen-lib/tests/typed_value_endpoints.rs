//! Stage-2 typed value endpoints, end to end:
//!
//! A graph value input/output declared with a non-f32 type
//! (`input mode: value: FilterMode;`) is a real field of that type with a
//! typed setter. Typed values are connected per-frame by copy, excluded
//! from the param registry (only f32 params get `{Graph}Param` variants,
//! descriptors, and `set_param` dispatch), and latch across rate
//! boundaries (one copy at the outer-block boundary).
#![feature(inherent_associated_types)]

use oscen::graph::ValuePayload;
use oscen::{graph, Node, SignalProcessor};

// ---------------------------------------------------------------------------
// A custom typed payload and a node whose behaviour depends on it.
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Default, Debug, PartialEq)]
pub enum FilterMode {
    #[default]
    Lowpass,
    Highpass,
    Bandpass,
}

impl ValuePayload for FilterMode {}

impl FilterMode {
    fn sign(self) -> f32 {
        match self {
            FilterMode::Lowpass => 1.0,
            FilterMode::Highpass => -1.0,
            FilterMode::Bandpass => 0.5,
        }
    }
}

#[derive(Debug, Node)]
pub struct ModeShaper {
    #[input(value)]
    pub mode: FilterMode,
    #[input(value)]
    pub gain: f32,
    #[input(stream)]
    pub input: f32,
    #[output(value)]
    pub active_mode: FilterMode,
    #[output(stream)]
    pub out: f32,
}

impl ModeShaper {
    pub fn new() -> Self {
        Self::with_mode(FilterMode::Lowpass)
    }

    pub fn with_mode(mode: FilterMode) -> Self {
        Self {
            mode,
            gain: 1.0,
            input: 0.0,
            active_mode: FilterMode::Lowpass,
            out: 0.0,
        }
    }
}

impl Default for ModeShaper {
    fn default() -> Self {
        Self::new()
    }
}

impl SignalProcessor for ModeShaper {
    #[inline(always)]
    fn process(&mut self) {
        self.active_mode = self.mode;
        self.out = self.input * self.gain * self.mode.sign();
    }
}

// ---------------------------------------------------------------------------
// Same-rate: typed input drives behaviour, typed output reads back.
// ---------------------------------------------------------------------------

graph! {
    name: TypedGraph;

    input mode: value: FilterMode;
    input value gain = 0.5;
    input stream sig;

    output active: value: FilterMode;
    output stream out;

    nodes {
        shaper = ModeShaper::new();
    }

    connections {
        mode -> shaper.mode;
        gain -> shaper.gain;
        sig -> shaper.input;
        shaper.active_mode -> active;
        shaper.out -> out;
    }
}

#[test]
fn typed_input_defaults_to_payload_default_and_drives_behaviour() {
    let mut g = TypedGraph::new();
    g.init(48_000.0);

    // No `= default` on the typed input: starts at FilterMode::default().
    assert_eq!(g.mode, FilterMode::Lowpass);
    assert_eq!(g.active, FilterMode::Lowpass);

    g.sig = 1.0;
    g.process();
    assert_eq!(g.active, FilterMode::Lowpass);
    assert_eq!(g.out, 0.5); // 1.0 * gain 0.5 * lowpass sign 1.0

    // The typed setter takes the payload type directly.
    g.set_mode(FilterMode::Highpass);
    g.process();
    assert_eq!(g.active, FilterMode::Highpass);
    assert_eq!(g.out, -0.5); // sign flipped by the typed value
}

#[test]
fn registry_partition_covers_only_f32_params() {
    // Only `gain` is a parameter; the typed input has no variant,
    // descriptor, or dispatch arm.
    assert_eq!(TypedGraphParam::COUNT, 1);
    assert_eq!(TypedGraphParam::ALL, [TypedGraphParam::Gain]);
    assert_eq!(
        TypedGraphParam::from_name("gain"),
        Some(TypedGraphParam::Gain)
    );
    assert_eq!(TypedGraphParam::from_name("mode"), None);

    let descs = TypedGraph::param_descriptors();
    assert_eq!(descs.len(), 1);
    assert_eq!(descs[0].name, "gain");
    assert_eq!(descs[0].default, 0.5);

    // set_param drives the f32 param; the typed setter coexists.
    let mut g = TypedGraph::new();
    g.init(48_000.0);
    g.set_param(TypedGraphParam::Gain, 0.25);
    assert_eq!(g.get_param(TypedGraphParam::Gain), 0.25);
    g.set_mode(FilterMode::Bandpass);
    g.sig = 1.0;
    g.process();
    assert_eq!(g.active, FilterMode::Bandpass);
    assert_eq!(g.out, 0.125); // 1.0 * 0.25 * bandpass sign 0.5
}

// ---------------------------------------------------------------------------
// Explicit typed default and typed hoist inheritance.
// ---------------------------------------------------------------------------

graph! {
    name: TypedDefaults;

    // Typed input with an explicit `= expr` initializer, used as-is.
    input mode: value: FilterMode = FilterMode::Bandpass;

    output stream out;

    nodes {
        shaper = ModeShaper::new();
    }

    // Explicit hoist of a typed child endpoint: the `: value: FilterMode`
    // annotation types the graph input; without a `= default` the initial
    // value is inherited from the child constructor.
    input shaper.gain shaper_gain;

    connections {
        mode -> shaper.mode;
        shaper.out -> out;
    }
}

graph! {
    name: TypedHoist;

    output stream out;

    nodes {
        shaper = ModeShaper::with_mode(FilterMode::Highpass);
    }

    // Typed hoist: declares `mode: FilterMode` on the graph and inherits
    // the child's constructor value (Highpass) via ReadValueEndpoint.
    input shaper.mode: value: FilterMode;

    connections {
        shaper.out -> out;
    }
}

#[test]
fn typed_default_expr_is_used_as_is() {
    let g = TypedDefaults::new();
    assert_eq!(g.mode, FilterMode::Bandpass);
    // The f32 hoist still inherits from the constructor (gain = 1.0).
    assert_eq!(g.shaper_gain, 1.0);
}

#[test]
fn typed_hoist_inherits_child_constructor_value() {
    let mut g = TypedHoist::new();
    assert_eq!(g.mode, FilterMode::Highpass);

    g.init(48_000.0);
    g.process();
    assert_eq!(g.shaper.mode, FilterMode::Highpass);

    // The hoisted typed setter retargets the child through the synthesized
    // connection.
    g.set_mode(FilterMode::Bandpass);
    g.process();
    assert_eq!(g.shaper.mode, FilterMode::Bandpass);
}

// ---------------------------------------------------------------------------
// Wildcard hoists inherit typed value endpoints through the manifest:
// (a) from a #[derive(Node)] child — the manifest carries the field's
// literal type tokens (`FilterMode` must be in scope here; it is).
// ---------------------------------------------------------------------------

graph! {
    name: TypedWildDerive;

    input stream sig;
    output stream out;

    nodes {
        shaper = ModeShaper::new();
    }

    // Hoists `mode` (value, FilterMode) and `gain` (value, f32); `input`
    // is a connection destination, so the wildcard skips it.
    input shaper.*;

    connections {
        sig -> shaper.input;
        shaper.out -> out;
    }
}

#[test]
fn wildcard_hoist_keeps_derive_child_typed_value() {
    let mut g = TypedWildDerive::new();
    g.init(48_000.0);

    // Initial value inherited from the child's constructor.
    assert_eq!(g.mode, FilterMode::Lowpass);

    // The hoisted typed setter drives the child through the synthesized
    // connection; the f32 sibling hoists as a plain param.
    g.set_mode(FilterMode::Highpass);
    g.set_gain(2.0);
    g.sig = 1.0;
    g.process();
    assert_eq!(g.shaper.mode, FilterMode::Highpass);
    assert_eq!(g.out, -2.0); // 1.0 * gain 2.0 * highpass sign -1.0
}

#[test]
fn wildcard_hoisted_typed_value_stays_out_of_registry() {
    assert_eq!(TypedWildDeriveParam::COUNT, 1);
    assert_eq!(TypedWildDeriveParam::ALL, [TypedWildDeriveParam::Gain]);
    assert_eq!(TypedWildDeriveParam::from_name("mode"), None);
    let descs = TypedWildDerive::param_descriptors();
    assert_eq!(descs.len(), 1);
    assert_eq!(descs[0].name, "gain");
}

// ---------------------------------------------------------------------------
// (b) from a nested graph! child — the inner graph's manifest re-exports
// its typed input's declared type tokens.
// ---------------------------------------------------------------------------

graph! {
    name: InnerTyped;

    input mode: value: FilterMode;
    input value level = 0.75;
    input stream sig;
    output stream out;

    nodes {
        shaper = ModeShaper::new();
    }

    connections {
        mode -> shaper.mode;
        level -> shaper.gain;
        sig -> shaper.input;
        shaper.out -> out;
    }
}

graph! {
    name: OuterTyped;

    output stream out;

    nodes {
        v = InnerTyped::new();
    }

    // Hoists `mode` (value, FilterMode), `level` (value, f32), and `sig`
    // (stream) from the inner graph's declared inputs.
    input v.*;

    connections {
        v.out -> out;
    }
}

#[test]
fn wildcard_hoist_keeps_nested_graph_typed_value() {
    let mut g = OuterTyped::new();
    g.init(48_000.0);

    assert_eq!(g.mode, FilterMode::Lowpass);
    assert_eq!(g.level, 0.75);

    g.set_mode(FilterMode::Bandpass);
    g.sig = 1.0;
    g.process();
    assert_eq!(g.v.shaper.mode, FilterMode::Bandpass);
    assert_eq!(g.out, 0.375); // 1.0 * level 0.75 * bandpass sign 0.5

    // Registry: only the f32 param.
    assert_eq!(OuterTypedParam::ALL, [OuterTypedParam::Level]);
    assert_eq!(OuterTypedParam::from_name("mode"), None);
}

// ---------------------------------------------------------------------------
// Endpoint-list hoists inherit typed value endpoints when the node's
// manifest is resolved (here the wildcard forces resolution). A list
// hoist on a node with no resolved manifest expands untyped (f32) —
// hoist typed endpoints explicitly (`input node.ep: value: T;`) in that
// case.
// ---------------------------------------------------------------------------

graph! {
    name: TypedListGraph;

    input stream sig;
    output stream out;

    nodes {
        shaper = ModeShaper::new();
    }

    input shaper.{mode, gain} cfg_*;
    input shaper.*;

    connections {
        sig -> shaper.input;
        shaper.out -> out;
    }
}

#[test]
fn list_hoist_keeps_typed_value_when_manifest_is_resolved() {
    let mut g = TypedListGraph::new();
    g.init(48_000.0);

    assert_eq!(g.cfg_mode, FilterMode::Lowpass);
    g.set_cfg_mode(FilterMode::Highpass);
    g.set_cfg_gain(0.5);
    g.sig = 1.0;
    g.process();
    assert_eq!(g.shaper.mode, FilterMode::Highpass);
    assert_eq!(g.out, -0.5); // 1.0 * gain 0.5 * highpass sign -1.0

    // Registry: the renamed f32 hoist only; the typed one is excluded.
    assert_eq!(TypedListGraphParam::ALL, [TypedListGraphParam::CfgGain]);
    assert_eq!(TypedListGraphParam::from_name("cfg_mode"), None);
}

// ---------------------------------------------------------------------------
// Cross-rate: typed values latch at the outer-block boundary.
// ---------------------------------------------------------------------------

graph! {
    name: TypedOversampled;

    input mode: value: FilterMode;
    input stream sig;

    output active: value: FilterMode;
    output stream out;

    nodes {
        shaper = ModeShaper::new() * 2;
    }

    connections {
        mode -> shaper.mode;
        sig -> shaper.input;
        shaper.active_mode -> active;
        [sinc] shaper.out -> out;
    }
}

#[test]
fn typed_values_latch_across_rate_boundaries() {
    let mut g = TypedOversampled::new();
    g.init(48_000.0);

    g.set_mode(FilterMode::Highpass);
    g.sig = 1.0;
    g.process();

    // Up direction: the inner (2x) node saw the latched typed input.
    assert_eq!(g.shaper.mode, FilterMode::Highpass);
    // Down direction: the last inner value latched out at the boundary.
    assert_eq!(g.active, FilterMode::Highpass);

    g.set_mode(FilterMode::Bandpass);
    g.process();
    assert_eq!(g.active, FilterMode::Bandpass);
}
