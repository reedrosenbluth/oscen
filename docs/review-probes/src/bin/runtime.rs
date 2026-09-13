//! Manual diagnostic for the September 2026 architecture review.
//! See `docs/ARCHITECTURE_REVIEW.md`; printed discrepancies describe the reviewed
//! revision, not behavior that future implementations must preserve.

#![feature(inherent_associated_types)]
#![allow(dead_code, non_camel_case_types)]
use oscen::{graph, AdsrEnvelope, EventPayload};
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
thread_local! { static COUNTS: Cell<(bool, usize, usize)> = const { Cell::new((false, 0, 0)) }; }
struct Counting;
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        let _ = COUNTS.try_with(|c| {
            let (on, a, d) = c.get();
            if on {
                c.set((on, a + 1, d));
            }
        });
        System.alloc(l)
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        let _ = COUNTS.try_with(|c| {
            let (on, a, d) = c.get();
            if on {
                c.set((on, a, d + 1));
            }
        });
        System.dealloc(p, l)
    }
}
#[global_allocator]
static ALLOC: Counting = Counting;
fn count(f: impl FnOnce()) -> (usize, usize) {
    COUNTS.with(|c| c.set((true, 0, 0)));
    f();
    COUNTS.with(|c| {
        let (_, a, d) = c.replace((false, 0, 0));
        (a, d)
    })
}
#[path = "../../../../oscen-lib/benches/support/poly_synth.rs"]
mod poly_synth;
graph! {
    name: ObjectSink;
    input gate: event;
    output out: stream;
    node env = AdsrEnvelope::new(0.01, 0.1, 0.5, 0.2);
    connections { gate -> env.gate; env.output -> out; }
}
fn main() {
    let (mut p, mut c) = oscen::handoff::pair::<u32>();
    p.publish(42);
    let ((a0, d0), (a, d), (a2, d2)) = std::thread::spawn(move || {
        // Idle take on a thread that has never touched arc-swap: one atomic
        // load, must not allocate. (A value is pending, but the flag is what
        // is consulted first only when nothing is pending; so probe the idle
        // path with a second consumer below.)
        let (_p_idle, mut c_idle) = oscen::handoff::pair::<u32>();
        let idle = count(|| {
            assert!(c_idle.take().is_none());
        });
        // First take *of a published value* on this thread: arc-swap may
        // claim its per-thread node here, once (documented).
        let first = count(|| {
            let v = c.take().unwrap();
            c.retire(v);
        });
        let second = count(|| {
            assert!(c.take().is_none());
        });
        (idle, first, second)
    })
    .join()
    .unwrap();
    println!("handoff fresh thread: idle take allocations={a0}, deallocations={d0} (expected 0,0); first pending take allocations={a}, deallocations={d} (documented: at most one arc-swap thread node); next empty take: allocations={a2}, deallocations={d2} (expected 0,0)");
    let mut g = ObjectSink::new();
    g.init(48000.0);
    assert!(g.push_gate(EventPayload::object([1u8; 16]), 0));
    let (a, d) = count(|| g.process_block(2));
    println!("preallocated Object event process_block(2): allocations={a}, deallocations={d}");
    println!("sizes: EventInstance={} EventPayload={} EventInput={} AdsrEnvelope={} MidiVoiceHandler={} VoiceAllocator8={} FmVoice={} PolySynth8={} PolySynth16={}",
       size_of::<oscen::EventInstance>(),size_of::<EventPayload>(),size_of::<oscen::EventInput>(),
       size_of::<AdsrEnvelope>(),size_of::<oscen::MidiVoiceHandler>(),size_of::<oscen::VoiceAllocator<8>>(),
       size_of::<poly_synth::FmVoice>(),size_of::<poly_synth::PolySynth8>(),size_of::<poly_synth::PolySynth16>());
}
