//! Manual diagnostic for the September 2026 architecture review.
//! See `docs/ARCHITECTURE_REVIEW.md`; printed discrepancies describe the reviewed
//! revision, not behavior that future implementations must preserve.

#![feature(inherent_associated_types)]
use oscen::{graph, Gain, SignalProcessor};
graph! { name: Example; input x: stream; output y: stream; node gain=Gain::new(2.0); connections {x->gain.input;gain.output->y;} }
graph! { name: Events; input e: event; output o: event; connections {e->o;} }
graph! { name: Ramped; input level: value=0.0 [ramp:4]; output y: stream; node gain=Gain::new(1.0); connections {level->gain.input;gain.output->y;} }
fn generic_tick<T: SignalProcessor>(g: &mut T) {
    g.process();
}
fn main() {
    let mut g = Example::new();
    g.init(48000.0);
    g.x = 3.0;
    generic_tick(&mut g);
    println!("SignalProcessor generic tick: y={} (expected 6)", g.y);
    g.process();
    println!("inherent tick: y={}", g.y);
    let mut g = Events::new();
    g.init(48000.0);
    assert!(g.push_e(1.0, 1));
    g.process_block(4);
    // The per-frame `o` reflects only the last frame; `o_block` accumulates
    // the whole block with block-relative offsets.
    println!(
        "event at frame1 after process_block(4): o_block count={} offset={:?} (expected 1, Some(1)); per-frame o count={} (last frame only)",
        g.o_block.len(),
        g.o_block.first().map(|e| e.frame_offset),
        g.o.len()
    );
    let mut a = Ramped::new();
    a.init(48000.0);
    a.level.set_with_ramp(1.0, 4);
    a.process_block(4);
    let mut b = Ramped::new();
    b.init(48000.0);
    b.set_level(1.0);
    b.process_block(4);
    println!(
        "direct public ramp current={}; generated setter current={} (expected both1)",
        a.level.current, b.level.current
    );
}
