//! Manual diagnostic for the September 2026 architecture review.
//! See `docs/ARCHITECTURE_REVIEW.md`; printed discrepancies describe the reviewed
//! revision, not behavior that future implementations must preserve.

#![feature(inherent_associated_types)]
#![allow(dead_code, non_camel_case_types)]
use oscen::{EventInstance, EventPayload};
use std::{hint::black_box, time::Instant};
#[path = "../../../../oscen-lib/benches/support/poly_synth.rs"]
mod poly_synth;
macro_rules! measure {
    ($ty:ty,$label:expr,$chord:expr) => {{
        let mut g = <$ty>::new();
        g.init(48000.0);
        if $chord {
            for (i, n) in [48, 52, 55, 59, 62, 65, 69, 72].iter().enumerate() {
                g.midi_in
                    .try_push(EventInstance {
                        frame_offset: i as u32,
                        payload: EventPayload::Midi([0x90, *n, 100]),
                    })
                    .unwrap();
            }
        }
        for _ in 0..100 {
            g.process_block(512);
            black_box(&g.out_block[..]);
        }
        let mut times = [0.0; 3];
        for t in &mut times {
            let start = Instant::now();
            for _ in 0..2000 {
                g.process_block(512);
                black_box(&g.out_block[..]);
            }
            *t = start.elapsed().as_secs_f64() * 1e6 / 2000.0;
        }
        println!("{} full-buffer observation us/block: {:?}", $label, times);
    }};
}
fn main() {
    measure!(poly_synth::PolySynth8, "idle8", false);
    measure!(poly_synth::PolySynth8, "chord8", true);
    measure!(poly_synth::PolySynth16, "idle16", false);
    measure!(poly_synth::PolySynth16, "chord16", true);
}
