//! Smoke test for the `oversample_variants!` proc-macro shim.
//!
//! Verifies that a single body can be expanded into three distinct graph
//! types differing only by their oversampling factor: `_1x` (same-rate),
//! `_2x`, and `_4x`.

#![allow(non_camel_case_types)]

use oscen::{oversample_variants, PolyBlepOscillator, SignalProcessor};

oversample_variants! {
    base_name: TestSynth;
    factors: [1, 2, 4];
    body: {
        output stream audio_out;
        nodes {
            osc = PolyBlepOscillator::saw(440.0, 0.6) * {FACTOR};
        }
        connections {
            [sinc] osc.output -> audio_out;
        }
    }
}

#[test]
fn three_variants_compile_and_init() {
    let mut a = TestSynth_1x::new();
    let mut b = TestSynth_2x::new();
    let mut c = TestSynth_4x::new();
    a.init(48_000.0);
    b.init(48_000.0);
    c.init(48_000.0);

    // Process a small block to ensure each variant runs end-to-end.
    a.process_block(64);
    b.process_block(64);
    c.process_block(64);
}
