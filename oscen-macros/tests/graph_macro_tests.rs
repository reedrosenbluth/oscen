use oscen::{graph, AdsrEnvelope, PolyBlepOscillator, TptFilter};

#[test]
fn test_basic_graph_parsing() {
    // This should compile if parsing works
    graph! {
        name: TestBasicGraph;

        input value cutoff = 3000.0 [range(20.0, 20000.0), log, ramp(1323)];
        input value volume = 0.8;
        input event gate;

        output stream out;

        node osc = PolyBlepOscillator::saw(440.0, 0.6);
        node filter = TptFilter::new(3000.0, 0.707);

        connection cutoff -> filter.cutoff();
        connection osc.output() -> filter.input();
        connection filter.output() -> out;
    }
}

#[test]
fn test_node_block_syntax() {
    graph! {
        name: TestNodeBlockGraph;

        input value freq = 440.0;
        output stream out;

        nodes {
            osc1 = PolyBlepOscillator::saw(440.0, 0.6);
            osc2 = PolyBlepOscillator::square(880.0, 0.4);
            filter = TptFilter::new(1000.0, 0.707);
        }

        connection {
            freq -> osc1.frequency();
            osc1.output() -> filter.input();
            filter.output() -> out;
        }
    }
}

#[test]
fn test_connection_block_syntax() {
    graph! {
        name: TestConnectionBlockGraph;

        input value cutoff = 1000.0;
        input value q = 0.707;
        output stream out;

        node filter = TptFilter::new(1000.0, 0.707);

        connections {
            cutoff -> filter.cutoff();
            q -> filter.q();
        }
    }
}

// #[test]
// #[ignore = "Arithmetic operations with ValueParam not yet implemented"]
// fn test_arithmetic_in_connections() {
//     graph! {
//         name: TestArithmeticGraph;
//
//         input value volume = 0.5;
//         output stream left;
//         output stream right;
//
//         node osc1 = PolyBlepOscillator::saw(440.0, 0.5);
//         node osc2 = PolyBlepOscillator::saw(441.0, 0.5);
//
//         connection {
//             osc1.output() * volume -> left;
//             osc2.output() * volume -> right;
//             osc1.output() + osc2.output() -> left;
//         }
//     }
// }

#[test]
fn test_mixed_syntax() {
    graph! {
        name: TestMixedSyntaxGraph;

        // Parameters
        input value cutoff = 3000.0 [range(20.0, 20000.0), log];
        input value q_factor = 0.707 [range(0.1, 10.0)];
        input value volume = 0.8;
        input event gate;

        output stream out;

        // Some individual nodes
        node osc = PolyBlepOscillator::saw(440.0, 0.6);

        // Node block
        nodes {
            filter = TptFilter::new(3000.0, 0.707);
            envelope = AdsrEnvelope::new(0.01, 0.1, 0.7, 0.2);
        }

        // Individual connection
        connection gate -> envelope.gate();

        // Connection block
        connection {
            cutoff -> filter.cutoff();
            q_factor -> filter.q();
            osc.output() -> filter.input();
            envelope.output() -> filter.f_mod();
            filter.output() * envelope.output() * volume -> out;
        }
    }
}

// #[test]
// #[ignore = "Arithmetic operations with ValueParam not yet implemented"]
// fn test_complex_arithmetic() {
//     graph! {
//         name: TestComplexArithmeticGraph;
//
//         input value mix = 0.5;
//         output stream out;
//
//         node osc1 = PolyBlepOscillator::saw(440.0, 0.5);
//         node osc2 = PolyBlepOscillator::square(440.0, 0.5);
//
//         connection {
//             osc1.output() * mix + osc2.output() * (1.0 - mix) -> out;
//         }
//     }
// }

#[test]
fn test_minimal_graph() {
    graph! {
        name: TestMinimalGraph;

        output stream out;
        node osc = PolyBlepOscillator::saw(440.0, 0.5);
        connection osc.output() -> out;
    }
}
