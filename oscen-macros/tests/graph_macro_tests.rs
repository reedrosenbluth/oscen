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

        nodes {
            osc = PolyBlepOscillator::saw(440.0, 0.6);
            filter = TptFilter::new(3000.0, 0.707);
        }

        connections {
            cutoff -> filter.cutoff;
            osc.output -> filter.input;
            filter.output -> out;
        }
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

        connections {
            // Use frequency_mod instead of frequency (private field)
            freq -> osc1.frequency_mod;
            osc1.output -> filter.input;
            filter.output -> out;
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

        nodes {
            filter = TptFilter::new(1000.0, 0.707);
        }

        connections {
            cutoff -> filter.cutoff;
            q -> filter.q;
        }
    }
}

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

        nodes {
            osc = PolyBlepOscillator::saw(440.0, 0.6);
            filter = TptFilter::new(3000.0, 0.707);
            envelope = AdsrEnvelope::new(0.01, 0.1, 0.7, 0.2);
        }

        connections {
            gate -> envelope.gate;
            cutoff -> filter.cutoff;
            q_factor -> filter.q;
            osc.output -> filter.input;
            envelope.output -> filter.f_mod;
            filter.output * envelope.output * volume -> out;
        }
    }
}

#[test]
fn test_minimal_graph() {
    graph! {
        name: TestMinimalGraph;

        output stream out;

        nodes {
            osc = PolyBlepOscillator::saw(440.0, 0.5);
        }

        connections {
            osc.output -> out;
        }
    }
}
