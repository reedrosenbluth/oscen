//! Smoke tests for parsing `* N` / `/ N` rate annotations.
//!
//! These verify the macro accepts the new syntax. Codegen still treats
//! everything as same-rate until Phase 4, so behavior here is not asserted —
//! only that the graph compiles and constructs.

use oscen::{graph, PolyBlepOscillator};

graph! {
    name: OversampleSmoke;
    output stream audio_out;
    nodes {
        osc = PolyBlepOscillator::saw(440.0, 0.6) * 4;
    }
    connections {
        osc.output -> audio_out;
    }
}

#[test]
fn graph_macro_with_oversample_compiles() {
    let mut g = OversampleSmoke::new();
    g.init(44100.0);
    let _ = g;
}

graph! {
    name: PolicySmoke;
    output stream audio_out;
    nodes {
        osc = PolyBlepOscillator::saw(440.0, 0.6) * 2;
    }
    connections {
        [sinc] osc.output -> audio_out;
    }
}

#[test]
fn graph_macro_with_connection_policy_compiles() {
    let mut g = PolicySmoke::new();
    g.init(44100.0);
}

#[test]
fn oversampled_init_does_not_panic() {
    // OversampleSmoke is defined earlier in this file with `osc * 4`.
    // The 4× node's init should be called with 4 × the host SR. Just confirm
    // it doesn't panic — full SR-scaling correctness is verified in Task 4.5.
    let mut g = OversampleSmoke::new();
    g.init(48_000.0);
    let _ = g;
}
