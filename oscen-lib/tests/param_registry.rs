//! Integration tests for the generated parameter registry:
//! `{Graph}Param` enum, `param_descriptors()`, and the
//! `set_param` / `set_param_immediate` / `get_param` dispatchers.

use oscen::{graph, PolyBlepOscillator, TptFilter};

graph! {
    name: RegistryGraph;

    input value gain = 0.5;
    input cutoff: value = 1000.0 [20.0..20000.0, log, unit = "Hz", ramp: 64];
    input drive: value = 2.0 [0.0..10.0, center = 2.0, step = 0.1, group = "Tone"];
    input gate: event;

    output stream out;

    nodes {
        osc = PolyBlepOscillator::saw(440.0, 0.6);
        filter = TptFilter::new(1000.0, 0.7);
    }

    connections {
        cutoff -> filter.cutoff;
        osc.output * gain * drive -> filter.input;
        filter.output -> out;
    }
}

#[test]
fn enum_reflects_declaration_order_and_names() {
    assert_eq!(RegistryGraphParam::COUNT, 3);
    assert_eq!(
        RegistryGraphParam::ALL,
        [
            RegistryGraphParam::Gain,
            RegistryGraphParam::Cutoff,
            RegistryGraphParam::Drive,
        ]
    );
    assert_eq!(RegistryGraphParam::Gain.index(), 0);
    assert_eq!(RegistryGraphParam::Cutoff.name(), "cutoff");
    assert_eq!(
        RegistryGraphParam::from_name("drive"),
        Some(RegistryGraphParam::Drive)
    );
    assert_eq!(RegistryGraphParam::from_name("gate"), None); // events excluded
    assert_eq!(RegistryGraphParam::from_name("nope"), None);
}

#[test]
fn descriptors_capture_spec_metadata() {
    let descs = RegistryGraph::param_descriptors();
    assert_eq!(descs.len(), 3);

    let gain = &descs[0];
    assert_eq!(gain.name, "gain");
    assert_eq!(gain.display_name, "Gain");
    assert_eq!(gain.default, 0.5);
    assert_eq!(gain.range, None);
    assert_eq!(gain.ramp_frames, None);
    assert!(!gain.logarithmic);

    let cutoff = RegistryGraphParam::Cutoff.descriptor();
    assert_eq!(cutoff.index, 1);
    assert_eq!(cutoff.range, Some((20.0, 20000.0)));
    assert_eq!(cutoff.unit, Some("Hz"));
    assert_eq!(cutoff.ramp_frames, Some(64));
    assert!(cutoff.logarithmic);

    let drive = RegistryGraphParam::Drive.descriptor();
    assert_eq!(drive.center, Some(2.0));
    assert_eq!(drive.step, Some(0.1));
    assert_eq!(drive.group, Some("Tone"));
}

#[test]
fn set_and_get_param_dispatch() {
    let mut g = RegistryGraph::new();
    g.init(48_000.0);

    // Plain value input: immediate effect.
    g.set_param(RegistryGraphParam::Gain, 0.8);
    assert_eq!(g.get_param(RegistryGraphParam::Gain), 0.8);
    assert_eq!(g.gain, 0.8);

    // Ramped input: set_param starts a ramp toward the target...
    g.set_param(RegistryGraphParam::Cutoff, 5000.0);
    assert_eq!(g.get_param(RegistryGraphParam::Cutoff), 5000.0); // target
    assert_ne!(g.cutoff.current, 5000.0); // still ramping

    // ...while set_param_immediate snaps.
    g.set_param_immediate(RegistryGraphParam::Cutoff, 2000.0);
    assert_eq!(g.cutoff.current, 2000.0);
    assert_eq!(g.get_param(RegistryGraphParam::Cutoff), 2000.0);
}

#[test]
fn generic_apply_loop() {
    // The pattern that replaces hand-written preset appliers: iterate a
    // (param, value) list and set everything through the enum.
    let preset: &[(RegistryGraphParam, f32)] = &[
        (RegistryGraphParam::Gain, 0.25),
        (RegistryGraphParam::Cutoff, 800.0),
        (RegistryGraphParam::Drive, 5.0),
    ];

    let mut g = RegistryGraph::new();
    g.init(48_000.0);
    for &(p, v) in preset {
        g.set_param_immediate(p, v);
    }
    assert_eq!(g.get_param(RegistryGraphParam::Drive), 5.0);
    assert_eq!(g.cutoff.current, 800.0);

    // Validate against descriptors (what a preset loader would do).
    for &(p, v) in preset {
        if let Some((min, max)) = p.descriptor().range {
            assert!(v >= min && v <= max, "{} out of range", p.name());
        }
    }
}
