//! Regression tests: wildcard hoists (`input node.*;`) must work under the
//! documented `use oscen::prelude::*;` idiom, not just `use oscen::*;`.
//!
//! The endpoint set comes from the node type's manifest macro
//! (`__oscen_endpoints_<Type>!`), which a bare constructor path resolves to a
//! bare invocation — so the prelude must re-export the manifest alias
//! alongside each node type name it exports.

use oscen::prelude::*;

// ---------------------------------------------------------------------------
// Wildcard on a prelude-exported #[derive(Node)] type.
// ---------------------------------------------------------------------------

graph! {
    name: PreludeWild;

    output stream out;

    nodes {
        osc = PolyBlepOscillator::saw(440.0, 0.6);
        filter = TptFilter::new(2000.0, 0.7);
    }

    // Both wildcards expand through the prelude-exported manifest aliases.
    input osc.*;
    input filter.*;

    connections {
        osc.output -> filter.input;
        filter.output -> out;
    }
}

#[test]
fn prelude_wildcard_hoists_value_inputs_with_child_defaults() {
    let g = PreludeWild::new();
    // Value hoists inherit their initial values from the constructors.
    assert_eq!(g.frequency, 440.0);
    assert_eq!(g.amplitude, 0.6);
    assert_eq!(g.q, 0.7);
}

#[test]
fn prelude_wildcard_hoists_forward_into_children() {
    let mut g = PreludeWild::new();
    g.init(48_000.0);

    g.set_frequency(220.0);
    g.set_q(0.9);
    g.process();
    assert_eq!(g.osc.frequency, 220.0);
    assert_eq!(g.filter.q, 0.9);
}

// ---------------------------------------------------------------------------
// Event inputs hoist through a prelude wildcard too.
// ---------------------------------------------------------------------------

graph! {
    name: PreludeWildEvents;

    output value freq_out;

    nodes {
        handler = MidiVoiceHandler::new();
    }

    input handler.*;

    connections {
        handler.frequency -> freq_out;
    }
}

#[test]
fn prelude_wildcard_hoists_event_inputs() {
    let mut g = PreludeWildEvents::new();
    g.init(48_000.0);

    assert!(g.push_note_on(oscen::EventPayload::Midi([0x90, 69, 100]), 0));
    g.process();
    assert_eq!(g.freq_out, 440.0);
}
