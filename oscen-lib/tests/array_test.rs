use oscen::*;

#[test]
fn test_array_node_creation() {
    graph! {
        name: ArrayTest;

        output stream out;

        node {
            oscs = [PolyBlepOscillator::saw(440.0, 0.6); 4];
        }

        connection {
            oscs[0].output() -> out;
        }
    }

    let graph = ArrayTest::new(48000.0);

    // Verify the graph was created successfully
    assert_eq!(graph.graph.sample_rate, 48000.0);
}

#[test]
fn test_array_connections() {
    graph! {
        name: ArrayConnectionTest;

        input value freq = 440.0;
        output stream out;

        node {
            oscs = [PolyBlepOscillator::saw(440.0, 0.6); 4];
        }

        connection {
            freq -> oscs[0].frequency();
            freq -> oscs[1].frequency();
            freq -> oscs[2].frequency();
            freq -> oscs[3].frequency();

            oscs[0].output() + oscs[1].output() + oscs[2].output() + oscs[3].output() -> out;
        }
    }

    let mut graph = ArrayConnectionTest::new(48000.0);

    // Set frequency
    graph.graph.set_value(graph.freq, 880.0);

    // Process a few samples
    for _ in 0..10 {
        graph.graph.process().unwrap();
    }
}
