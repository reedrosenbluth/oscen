//! Manual diagnostic for the September 2026 architecture review.
//! See `docs/ARCHITECTURE_REVIEW.md`; printed discrepancies describe the reviewed
//! revision, not behavior that future implementations must preserve.

#![feature(inherent_associated_types)]
#![allow(dead_code)]
use oscen::{graph, Convolver, EventInput, EventInstance, Gain, Node, SignalProcessor};
// `Val` and `Counter` below are kept as the node shapes the removed cases used.
#[derive(Debug, Default, Node)]
pub struct Sink {
    #[input(event)]
    pub ev: EventInput,
    pub a: u32,
    pub b: u32,
}
impl Sink {
    pub fn new() -> Self {
        Self::default()
    }
    fn on_ev(&mut self, e: &EventInstance) {
        match e.payload.as_scalar() {
            Some(1.0) => self.a += 1,
            Some(2.0) => self.b += 1,
            _ => (),
        }
    }
}
impl SignalProcessor for Sink {
    fn process(&mut self) {}
}
graph! {name: EventMerge; input a: event; input b: event; node sink=Sink::new(); connections {a->sink.ev; b->sink.ev;}}
graph! {name: Indexed; input gain: value = 2.0; output out: stream; node voices=[Gain::new(1.0);4]; connections {gain*0.5->voices[2].input; voices[2].output->out;}}
#[derive(Debug, Node)]
pub struct Val {
    #[input(value)]
    pub input: f32,
    #[output(value)]
    pub output: f32,
}
impl Val {
    pub fn new(input: f32) -> Self {
        Self { input, output: 0.0 }
    }
}
impl SignalProcessor for Val {
    fn process(&mut self) {
        self.output = self.input;
    }
}
// `ValueMerge` (two unanchored value outputs into one value input) is now a
// rustc error through `FanInAllowed`; see
// oscen-macros/tests/ui/value_fanin_unknown_kind.rs.
graph! {name: WrongAsset; external impulse: DoesNotExist; output out: stream; node reverb=Convolver::new(); connections {impulse->reverb.typo;reverb.output->out;}}
#[derive(Debug, Node)]
pub struct Counter {
    #[output(stream)]
    pub output: f32,
}
impl Counter {
    pub fn new() -> Self {
        Self { output: 0.0 }
    }
}
impl SignalProcessor for Counter {
    fn process(&mut self) {
        self.output += 1.0;
    }
}
// `SlowFirst` / `FastFirst` (an expression mixing a base-rate and an
// oversampled node, in either operand order) are now lowering errors; see
// oscen-macros/tests/ui/mixed_rate_expression.rs.
fn main() {
    let mut g = EventMerge::new();
    g.init(48000.0);
    assert!(g.push_a(1.0, 0));
    assert!(g.push_b(2.0, 0));
    g.process();
    println!(
        "anchored event fanin received a={}, b={} (expected 1,1)",
        g.sink.a, g.sink.b
    );
    let mut g = Indexed::new();
    g.init(48000.0);
    g.process();
    println!(
        "indexed compound destination inputs={:?} (expected [0,0,1,0])",
        g.voices.each_ref().map(|v| v.input)
    );
    let _g = WrongAsset::new();
    println!("external type DoesNotExist + reverb.typo compiled (expected compile error)");
}
