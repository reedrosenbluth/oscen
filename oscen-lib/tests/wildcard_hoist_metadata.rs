//! Wildcard hoists (`input node.*;`) preserve the endpoint metadata the
//! manifest carries:
//!
//! - a child stream endpoint typed `Frame<2>` hoists as a `Frame<2>`
//!   parent input (not mono f32), for both `#[derive(Node)]` children
//!   (literal field-type tokens) and `graph!` children (fully-qualified
//!   frame path);
//! - a child `graph!` input declared `[ramp: N]` hoists as a parent input
//!   with the same declared ramp, so the smoothing survives the hoist;
//! - a `#[derive(Node)]` child storing a value input as `ValueRampState`
//!   exports a `ramped` manifest marker (the compile error that marker
//!   produces under a wildcard is covered by the `wildcard_ramped_field`
//!   UI test in `oscen-macros`).
#![feature(inherent_associated_types)]

use oscen::graph::ValueRampState;
use oscen::{graph, Frame, Node, PolyBlepOscillator, SignalProcessor};

const RATE: f32 = 48_000.0;

// ---------------------------------------------------------------------------
// Frame-typed stream hoist through a #[derive(Node)] child: the manifest
// carries the field's literal type tokens (`Frame<2>` must be in scope
// here — it is, via the glob import).
// ---------------------------------------------------------------------------

/// Stereo pass-through with a scalar gain.
#[derive(Debug, Node)]
pub struct StereoPass {
    #[input(stream)]
    pub inp: Frame<2>,
    #[input(value)]
    pub gain: f32,
    #[output(stream)]
    pub out: Frame<2>,
}

impl StereoPass {
    pub fn new() -> Self {
        Self {
            inp: Frame([0.0; 2]),
            gain: 1.0,
            out: Frame([0.0; 2]),
        }
    }
}

impl Default for StereoPass {
    fn default() -> Self {
        Self::new()
    }
}

impl SignalProcessor for StereoPass {
    #[inline(always)]
    fn process(&mut self) {
        self.out = self.inp * self.gain;
    }
}

graph! {
    name: WildStereoDerive;

    output stream wet: Frame<2>;

    nodes {
        g = StereoPass::new();
    }

    // Hoists `inp` (stream, Frame<2>) and `gain` (value, f32).
    input g.*;

    connections {
        g.out -> wet;
    }
}

#[test]
fn wildcard_hoist_keeps_derive_child_frame_type() {
    let mut g = WildStereoDerive::new();
    g.init(RATE);

    // The hoisted stream input is a Frame<2> field: distinct per-channel
    // audio passes through unmixed.
    g.inp = Frame([0.5, -0.25]);
    g.set_gain(2.0);
    g.process();
    assert_eq!(g.wet.0, [1.0, -0.5]);
}

// ---------------------------------------------------------------------------
// Frame-typed stream hoist through a nested graph! child: the graph's
// manifest canonicalizes the annotation to `::oscen::frame::Frame<2>`.
// ---------------------------------------------------------------------------

graph! {
    name: InnerStereo;

    input stream dry: Frame<2>;
    output stream wet: Frame<2>;

    nodes {
        g = StereoPass::new();
    }

    connections {
        dry -> g.inp;
        g.out -> wet;
    }
}

graph! {
    name: OuterStereo;

    output stream wet: Frame<2>;

    nodes {
        v = InnerStereo::new();
    }

    // Hoists `dry` (stream, Frame<2>) and the inner graph's own hoisted
    // value input `gain`... none declared: just `dry`.
    input v.*;

    connections {
        v.wet -> wet;
    }
}

#[test]
fn wildcard_hoist_keeps_nested_graph_frame_type() {
    let mut g = OuterStereo::new();
    g.init(RATE);

    g.dry = Frame([0.25, -0.75]);
    g.process();
    assert_eq!(g.wet.0, [0.25, -0.75]);
}

// ---------------------------------------------------------------------------
// Ramped value hoist through a nested graph! child: the child's declared
// `[ramp: 4]` re-declares on the hoisted parent input, so smoothing is
// observable at the parent (intermediate value mid-ramp) instead of being
// silently stripped.
// ---------------------------------------------------------------------------

graph! {
    name: InnerRamp;

    input value cutoff = 100.0 [ramp: 4];
    output stream audio;

    nodes {
        osc = PolyBlepOscillator::sine(100.0, 0.5);
    }

    connections {
        cutoff -> osc.frequency;
        osc.output -> audio;
    }
}

graph! {
    name: OuterRamp;

    output stream out;

    nodes {
        v = InnerRamp::new();
    }

    input v.*;

    connections {
        v.audio -> out;
    }
}

#[test]
fn wildcard_hoist_inherits_declared_ramp() {
    // The hoisted input is stored as a ramp state seeded from the child's
    // default, and the registry entry carries the child's ramp length.
    let g = OuterRamp::new();
    assert_eq!(g.cutoff.current, 100.0);
    let p = OuterRampParam::from_name("cutoff").expect("cutoff registered");
    assert_eq!(p.descriptor().ramp_frames, Some(4));
}

#[test]
fn wildcard_hoist_ramp_smoothing_is_observable() {
    let mut g = OuterRamp::new();
    g.init(RATE);

    g.set_cutoff(200.0);
    // One frame in: mid-ramp, strictly between start and target...
    g.process();
    assert!(
        g.cutoff.current > 100.0 && g.cutoff.current < 200.0,
        "expected an intermediate ramp value, got {}",
        g.cutoff.current
    );
    // ...and the child follows the parent's smoothed value per frame.
    assert_eq!(g.v.cutoff.current, g.cutoff.current);

    // After the declared 4 frames the target is reached, all the way down
    // to the inner oscillator.
    for _ in 0..3 {
        g.process();
    }
    assert_eq!(g.cutoff.current, 200.0);
    assert_eq!(g.v.cutoff.current, 200.0);
    assert_eq!(g.v.osc.frequency, 200.0);
}

// ---------------------------------------------------------------------------
// Manifest metadata probes: `ty = …` on frame-typed derive endpoints,
// `ramped` on ValueRampState fields.
// ---------------------------------------------------------------------------

/// A voice smoothing its own level with a runtime-configured ramp: the
/// manifest can only say "ramped", not how long.
#[derive(Debug, Node)]
pub struct SmoothNode {
    #[input(value)]
    pub level: ValueRampState,
    #[output(stream)]
    pub out: f32,
}

macro_rules! collect_inputs {
    (
        $konst:ident
        node_type $ty:ident
        inputs [ $($in_name:ident : $in_kind:ident $(( $($in_meta:tt)* ))?),* ]
        outputs [ $($out_name:ident : $out_kind:ident $(( $($out_meta:tt)* ))?),* ]
    ) => {
        const $konst: &[(&str, &str, &str)] =
            &[ $( (
                stringify!($in_name),
                stringify!($in_kind),
                stringify!($($($in_meta)*)?),
            ) ),* ];
    };
}

__oscen_endpoints_SmoothNode!(collect_inputs => (SMOOTH_INPUTS));
__oscen_endpoints_StereoPass!(collect_inputs => (STEREO_INPUTS));
__oscen_endpoints_InnerRamp!(collect_inputs => (INNER_RAMP_INPUTS));
__oscen_endpoints_InnerStereo!(collect_inputs => (INNER_STEREO_INPUTS));

#[test]
fn derive_manifest_marks_ramp_state_fields_ramped() {
    assert_eq!(SMOOTH_INPUTS, &[("level", "value", "ramped")]);
}

#[test]
fn derive_manifest_carries_literal_frame_type_tokens() {
    assert_eq!(
        STEREO_INPUTS,
        &[("inp", "stream", "ty = Frame < 2 >"), ("gain", "value", "")]
    );
}

#[test]
fn graph_manifest_carries_ramp_length_and_qualified_frame_type() {
    assert_eq!(INNER_RAMP_INPUTS, &[("cutoff", "value", "ramp = 4")]);
    assert_eq!(
        INNER_STEREO_INPUTS,
        &[("dry", "stream", "ty = :: oscen :: frame :: Frame < 2 >")]
    );
}
