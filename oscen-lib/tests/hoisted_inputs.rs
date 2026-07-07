//! Integration tests for hoisted endpoint declarations (Phase 2, Cmajor-style):
//!
//! ```ignore
//! input <node>.<endpoint> [rename] [= default [spec]];
//! ```
//!
//! A hoist declares a graph input *and* synthesizes the connection to the
//! child endpoint. Without `= default`, the initial value is inherited from
//! the constructed child node (single source of truth stays with the child).

use oscen::{graph, PolyBlepOscillator, TptFilter};

// ---------------------------------------------------------------------------
// Plain node hoists: rename, metadata, default inheritance from constructor.
// ---------------------------------------------------------------------------

graph! {
    name: HoistBasic;

    output stream out;

    nodes {
        osc = PolyBlepOscillator::saw(440.0, 0.6);
        filter = TptFilter::new(1200.0, 0.7);
    }

    // Hoist without default: inherits 440.0 from the constructor.
    input osc.frequency;

    // Hoist + rename + explicit default + metadata (feeds param registry).
    input osc.amplitude level = 0.5 [0.0..1.0, group = "Mix"];

    // Hoist a value input with a ramp spec.
    input filter.q resonance = 0.7 [0.1..10.0, ramp: 8];

    connections {
        osc.output -> filter.input;
        filter.output -> out;
    }
}

#[test]
fn hoist_inherits_default_from_child_constructor() {
    let g = HoistBasic::new();
    assert_eq!(g.frequency, 440.0); // from PolyBlepOscillator::saw(440.0, ..)
    assert_eq!(g.level, 0.5); // explicit default wins
}

#[test]
fn hoisted_inputs_generate_setters_and_forward() {
    let mut g = HoistBasic::new();
    g.init(48_000.0);

    g.set_frequency(220.0);
    g.set_level(1.0);
    g.process();
    assert_eq!(g.osc.frequency, 220.0);
    assert_eq!(g.osc.amplitude, 1.0);

    // Ramped hoist: target reached after the declared 8 frames.
    g.set_resonance(2.0);
    for _ in 0..8 {
        g.process();
    }
    assert!((g.filter.q - 2.0).abs() < 1e-6);
}

#[test]
fn hoisted_inputs_join_param_registry() {
    // Hoists are declared inputs: they get registry entries with metadata.
    let level = HoistBasicParam::from_name("level").expect("renamed hoist registered");
    let d = level.descriptor();
    assert_eq!(d.range, Some((0.0, 1.0)));
    assert_eq!(d.group, Some("Mix"));

    let res = HoistBasicParam::from_name("resonance").expect("resonance registered");
    assert_eq!(res.descriptor().ramp_frames, Some(8));

    let mut g = HoistBasic::new();
    g.init(48_000.0);
    g.set_param_immediate(HoistBasicParam::Level, 0.25);
    g.process();
    assert_eq!(g.osc.amplitude, 0.25);
}

// ---------------------------------------------------------------------------
// Array broadcast: hoisting through a node array reuses input -> voices.x
// fan-out (write to every element).
// ---------------------------------------------------------------------------

graph! {
    name: HoistArray;

    output stream out;

    nodes {
        voices = [PolyBlepOscillator::saw(110.0, 0.2); 4];
    }

    input voices.amplitude gain;

    connections {
        voices.output -> out;
    }
}

#[test]
fn hoist_through_array_broadcasts() {
    let mut g = HoistArray::new();
    g.init(48_000.0);

    // Inherited initial value comes from element 0's constructor.
    assert_eq!(g.gain, 0.2);

    g.set_gain(0.9);
    g.process();
    for v in &g.voices {
        assert_eq!(v.amplitude, 0.9);
    }
}

// ---------------------------------------------------------------------------
// Nested graph hoist: re-exporting a child graph's (ramped) input. The child
// input's storage is a ValueRampState; ReadValueEndpoint handles inheritance
// and the f32 -> ValueRampState ConnectEndpoints impl handles forwarding.
// ---------------------------------------------------------------------------

graph! {
    name: InnerVoice;

    input value tune = 330.0 [20.0..2000.0, ramp: 4];
    output stream audio;

    nodes {
        osc = PolyBlepOscillator::sine(330.0, 0.5);
    }

    connections {
        tune -> osc.frequency;
        osc.output -> audio;
    }
}

graph! {
    name: HoistNested;

    output stream out;

    nodes {
        voice = InnerVoice;
    }

    input voice.tune;

    connections {
        voice.audio -> out;
    }
}

#[test]
fn hoist_through_nested_graph() {
    let mut g = HoistNested::new();
    g.init(48_000.0);

    // Inherited from InnerVoice's declared default (its ramp state's current).
    assert_eq!(g.tune, 330.0);

    g.set_tune(550.0);
    // Parent input is unramped; child ramp state is driven per-frame via
    // set_immediate, so the child's oscillator follows on the next frame.
    g.process();
    assert_eq!(g.voice.tune.current, 550.0);
    g.process();
    assert_eq!(g.voice.osc.frequency, 550.0);
}
