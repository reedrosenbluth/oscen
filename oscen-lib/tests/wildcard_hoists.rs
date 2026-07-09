//! Integration tests for wildcard hoists (`input node.*;`).
//!
//! A wildcard hoists every (pub) input endpoint of a node — value, stream,
//! and event kinds — skipping endpoints that are already connection
//! destinations or already hoisted explicitly. The endpoint set comes from
//! the node type's exported manifest macro (`__oscen_endpoints_<Type>!`),
//! resolved through a two-stage continuation-passing expansion; the
//! manifest must be in scope wherever the node type is named bare (a glob
//! import `use oscen::*` or a qualified constructor path both work).

use oscen::*;

// ---------------------------------------------------------------------------
// Wildcard on a #[derive(Node)] node.
// ---------------------------------------------------------------------------

graph! {
    name: WildDerive;

    output stream out;

    nodes {
        lfo = PolyBlepOscillator::sine(2.0, 1.0);
        osc = PolyBlepOscillator::saw(440.0, 0.6);
    }

    // Hoist everything not otherwise claimed: `frequency` and the stream
    // inputs `phase_mod` / `frequency_mod` (pub inputs of the oscillator).
    input osc.*;

    // Explicit hoist with rename + metadata: the wildcard skips `amplitude`.
    input osc.amplitude level = 0.5 [0.0..1.0];

    connections {
        // `phase_mod` is a connection destination: the wildcard skips it.
        lfo.output -> osc.phase_mod;
        osc.output -> out;
    }
}

#[test]
fn wildcard_hoists_unclaimed_inputs_with_child_defaults() {
    let g = WildDerive::new();
    // Value hoist inherits its initial value from the constructor.
    assert_eq!(g.frequency, 440.0);
    // Explicit hoist coexists (with its own default).
    assert_eq!(g.level, 0.5);
}

#[test]
fn wildcard_skips_connected_and_explicitly_hoisted_endpoints() {
    let mut g = WildDerive::new();
    g.init(48_000.0);

    // Hoisted value input forwards into the child.
    g.set_frequency(220.0);
    g.process();
    assert_eq!(g.osc.frequency, 220.0);

    // `phase_mod` was NOT hoisted (lfo drives it): after processing, the
    // child's phase_mod carries the lfo output, not a graph input.
    // (Compile-time check: the graph has no `phase_mod` setter — this
    // would fail to compile if the wildcard had hoisted it:)
    // g.set_phase_mod(0.0);

    // `amplitude` was hoisted explicitly as `level`; the wildcard skipped it.
    g.set_level(1.0);
    g.process();
    assert_eq!(g.osc.amplitude, 1.0);
}

#[test]
fn wildcard_hoists_join_param_registry() {
    // Hoisted value inputs get param-registry entries (Phase-1 machinery).
    let freq = WildDeriveParam::from_name("frequency").expect("frequency registered");
    let mut g = WildDerive::new();
    g.init(48_000.0);
    g.set_param_immediate(freq, 330.0);
    g.process();
    assert_eq!(g.osc.frequency, 330.0);
    assert_eq!(g.get_param(freq), 330.0);

    // Stream hoists don't become params; value ones do.
    assert!(WildDeriveParam::from_name("frequency_mod").is_none());
    assert!(WildDeriveParam::from_name("level").is_some());
}

// ---------------------------------------------------------------------------
// Wildcard through a node ARRAY broadcasts (same as list hoists).
// ---------------------------------------------------------------------------

graph! {
    name: WildArray;

    output stream out;

    nodes {
        voices = [PolyBlepOscillator::saw(110.0, 0.2); 4];
    }

    input voices.*;

    connections {
        voices.output -> out;
    }
}

#[test]
fn wildcard_through_array_broadcasts() {
    let mut g = WildArray::new();
    g.init(48_000.0);

    // Initial value inherited from element 0's constructor.
    assert_eq!(g.amplitude, 0.2);

    g.set_amplitude(0.9);
    g.set_frequency(55.0);
    g.process();
    for v in &g.voices {
        assert_eq!(v.amplitude, 0.9);
        assert_eq!(v.frequency, 55.0);
    }
}

// ---------------------------------------------------------------------------
// Wildcard on a nested graph! node (hoist-through): the generated graph
// type exports the same manifest shape as derive(Node) types.
// ---------------------------------------------------------------------------

graph! {
    name: InnerVoice;

    input value pitch = 220.0;
    input value brightness = 2000.0 [100.0..8000.0];
    output stream audio_out;

    nodes {
        osc = PolyBlepOscillator::saw(220.0, 0.5);
        filter = TptFilter::new(2000.0, 0.7);
    }

    connections {
        pitch -> osc.frequency;
        brightness -> filter.cutoff;
        osc.output -> filter.input;
        filter.output -> audio_out;
    }
}

graph! {
    name: OuterSynth;

    output stream out;

    nodes {
        voice = InnerVoice::new();
    }

    // Hoists `pitch` and `brightness` through the nested graph.
    input voice.*;

    connections {
        voice.audio_out -> out;
    }
}

#[test]
fn wildcard_hoists_through_nested_graph() {
    let mut g = OuterSynth::new();
    g.init(48_000.0);

    // Defaults inherited from the inner graph's own input defaults.
    assert_eq!(g.pitch, 220.0);
    assert_eq!(g.brightness, 2000.0);

    g.set_pitch(440.0);
    g.set_brightness(500.0);
    g.process();
    assert_eq!(g.voice.pitch, 440.0);
    assert_eq!(g.voice.brightness, 500.0);
    // And through to the inner nodes.
    assert_eq!(g.voice.osc.frequency, 440.0);
}

#[test]
fn nested_wildcard_set_param_round_trip() {
    let pitch = OuterSynthParam::from_name("pitch").expect("pitch hoisted through");
    let brightness = OuterSynthParam::from_name("brightness").expect("brightness hoisted through");

    let mut g = OuterSynth::new();
    g.init(48_000.0);
    g.set_param_immediate(pitch, 660.0);
    g.set_param_immediate(brightness, 1234.0);
    g.process();
    assert_eq!(g.get_param(pitch), 660.0);
    assert_eq!(g.get_param(brightness), 1234.0);
    assert_eq!(g.voice.osc.frequency, 660.0);
}

// ---------------------------------------------------------------------------
// Manifest exported path: simulate cross-crate use by invoking the manifest
// through its path on the `oscen` crate (as a downstream crate would).
// ---------------------------------------------------------------------------

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

// Fully-qualified path, exactly what a downstream crate writes for a node
// declared as `osc = oscen::PolyBlepOscillator::saw(...)`.
::oscen::__oscen_endpoints_PolyBlepOscillator!(collect_manifest => (probe));

#[test]
fn manifest_reachable_through_crate_path_and_lists_all_endpoints() {
    assert_eq!(MANIFEST_TYPE, "PolyBlepOscillator");
    // Declaration order, kinds preserved. The private `pulse_width` input
    // is listed with a `priv` marker (it's a real endpoint — the marker
    // tells visibility apart from absence), and wildcard expansion skips
    // it (a parent graph cannot write a non-pub field).
    // Mono `f32` stream endpoints carry no `ty` annotation.
    assert_eq!(
        MANIFEST_INPUTS,
        &[
            ("phase_mod", "stream", ""),
            ("frequency", "value", ""),
            ("frequency_mod", "stream", ""),
            ("amplitude", "value", ""),
            ("pulse_width", "value", "priv"),
        ]
    );
    assert_eq!(MANIFEST_OUTPUTS, &[("output", "stream", "")]);
}

// ---------------------------------------------------------------------------
// Event inputs hoist through a wildcard too (all kinds, not just value).
// ---------------------------------------------------------------------------

graph! {
    name: WildEvents;

    output value freq_out;

    nodes {
        handler = MidiVoiceHandler::new();
    }

    // Hoists BOTH event inputs: note_on and note_off.
    input handler.*;

    connections {
        handler.frequency -> freq_out;
    }
}

#[test]
fn wildcard_hoists_event_inputs() {
    let mut g = WildEvents::new();
    g.init(48_000.0);

    // The hoisted event inputs generate push helpers; a note-on event
    // reaches the handler and retunes the frequency output.
    assert!(g.push_note_on(EventPayload::Midi([0x90, 69, 100]), 0));
    g.process();
    assert_eq!(g.freq_out, 440.0);

    assert!(g.push_note_on(EventPayload::Midi([0x90, 81, 100]), 0));
    g.process();
    assert_eq!(g.freq_out, 880.0);
}
