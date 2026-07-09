//! Regression test: `#[derive(Node)]` and `graph!` each export a
//! crate-global `#[macro_export]` endpoint-manifest macro. Its name hashes
//! the definition's tokens (`__oscen_endpoints_export_<Type>_<hash>`), so
//! two same-named types with different definitions in different modules of
//! one crate compile together — previously they collided on the exported
//! macro name (E0428) even for users who never touch wildcards.
//! Only byte-identical same-named definitions still collide (documented
//! limitation; see docs/COOKBOOK.md).
#![feature(inherent_associated_types)]

use oscen::graph::SignalProcessor;
use oscen::Node;

// ---------------------------------------------------------------------------
// Two same-named #[derive(Node)] types with different fields.
// ---------------------------------------------------------------------------

mod first {
    use super::*;

    #[derive(Debug, Node)]
    pub struct DupNode {
        #[input]
        pub frequency: f32,
        #[output(stream)]
        pub output: f32,
    }

    impl SignalProcessor for DupNode {
        fn process(&mut self) {
            self.output = self.frequency;
        }
    }
}

mod second {
    use super::*;

    #[derive(Debug, Node)]
    pub struct DupNode {
        #[input(stream)]
        pub input: f32,
        #[input]
        pub gain: f32,
        #[output(stream)]
        pub output: f32,
    }

    impl SignalProcessor for DupNode {
        fn process(&mut self) {
            self.output = self.input * self.gain;
        }
    }
}

#[test]
fn same_named_derive_nodes_in_two_modules_coexist() {
    let mut a = first::DupNode {
        frequency: 440.0,
        output: 0.0,
    };
    let mut b = second::DupNode {
        input: 0.5,
        gain: 2.0,
        output: 0.0,
    };
    a.process();
    b.process();
    assert_eq!(a.output, 440.0);
    assert_eq!(b.output, 1.0);
}

// ---------------------------------------------------------------------------
// Two same-named graph! types with different bodies.
// ---------------------------------------------------------------------------

mod graph_first {
    use oscen::*;

    graph! {
        name: DupGraph;

        input value freq = 220.0;
        output stream out;

        nodes {
            osc = PolyBlepOscillator::sine(220.0, 1.0);
        }

        connections {
            freq -> osc.frequency_mod;
            osc.output -> out;
        }
    }
}

mod graph_second {
    use oscen::*;

    graph! {
        name: DupGraph;

        input value level = 0.25;
        output stream out;

        nodes {
            osc = PolyBlepOscillator::saw(440.0, 0.6);
            amp = Gain::new(1.0);
        }

        connections {
            level -> amp.gain;
            osc.output -> amp.input;
            amp.output -> out;
        }
    }
}

#[test]
fn same_named_graphs_in_two_modules_coexist() {
    let mut a = graph_first::DupGraph::new();
    let mut b = graph_second::DupGraph::new();
    a.init(48_000.0);
    b.init(48_000.0);
    assert_eq!(a.freq, 220.0);
    assert_eq!(b.level, 0.25);
    a.process();
    b.process();
}
