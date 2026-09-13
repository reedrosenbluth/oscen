//! Regression tests for compound expressions driving one element of a node
//! array. `gain * 0.5 -> voices[2].input` used to broadcast to every
//! element: the compound-source path noticed the destination was an array
//! and ignored the selected index.
#![feature(inherent_associated_types)]

use oscen::{graph, Gain};

graph! {
    name: Indexed;
    input gain: value = 2.0;
    output out: stream;
    node voices = [Gain::new(1.0); 4];
    connections {
        gain * 0.5 -> voices[2].input;
        voices[2].output -> out;
    }
}

#[test]
fn compound_source_into_indexed_destination_touches_only_that_element() {
    let mut g = Indexed::new();
    g.init(48_000.0);
    g.process();
    let inputs: Vec<f32> = g.voices.iter().map(|v| v.input).collect();
    assert_eq!(inputs, vec![0.0, 0.0, 1.0, 0.0]);
    assert_eq!(g.out, 1.0);
}

// Metamorphic pair: wrapping a source in an identity expression must not
// change which slot it addresses.
graph! {
    name: Plain;
    input x: value = 3.0;
    output out: stream;
    node voices = [Gain::new(1.0); 4];
    connections {
        x -> voices[2].input;
        voices[2].output -> out;
    }
}

graph! {
    name: Identity;
    input x: value = 3.0;
    output out: stream;
    node voices = [Gain::new(1.0); 4];
    connections {
        x * 1.0 -> voices[2].input;
        voices[2].output -> out;
    }
}

#[test]
fn identity_expression_preserves_the_addressed_slot() {
    let mut a = Plain::new();
    let mut b = Identity::new();
    a.init(48_000.0);
    b.init(48_000.0);
    for _ in 0..4 {
        a.process();
        b.process();
    }
    let ia: Vec<f32> = a.voices.iter().map(|v| v.input).collect();
    let ib: Vec<f32> = b.voices.iter().map(|v| v.input).collect();
    assert_eq!(ia, ib);
    assert_eq!(ia, vec![0.0, 0.0, 3.0, 0.0]);
    assert_eq!(a.out, b.out);
}

graph! {
    name: IdentityOutput;
    input x: value = 3.0;
    output plain: stream;
    output wrapped: stream;
    node voices = [Gain::new(1.0); 4];
    connections {
        x -> voices[1].input;
        voices[1].output -> plain;
        voices[1].output * 1.0 -> wrapped;
    }
}

#[test]
fn identity_expression_on_an_indexed_source_reads_the_same_element() {
    let mut g = IdentityOutput::new();
    g.init(48_000.0);
    g.process();
    assert_eq!(g.plain, 3.0);
    assert_eq!(g.wrapped, 3.0);
}

// Oversampled voice array: the indexed destination is written through the
// cross-rate path, which must also address one element.
graph! {
    name: IndexedFast;
    input gain: value = 2.0;
    output out: stream;
    node voices = [Gain::new(1.0); 4] * 2;
    connections {
        gain * 0.5 -> voices[2].input;
        voices[2].output -> out;
    }
}

#[test]
fn compound_source_into_indexed_oversampled_destination_touches_only_that_element() {
    let mut g = IndexedFast::new();
    g.init(48_000.0);
    for _ in 0..4 {
        g.process();
    }
    let inputs: Vec<f32> = g.voices.iter().map(|v| v.input).collect();
    assert_eq!(inputs, vec![0.0, 0.0, 1.0, 0.0]);
}
