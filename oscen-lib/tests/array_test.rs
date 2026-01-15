use oscen::*;

#[test]
fn test_array_node_creation() {
    graph! {
        name: ArrayTest;

        output stream out;

        nodes {
            oscs = [PolyBlepOscillator::saw(440.0, 0.6); 4];
        }

        connections {
            oscs[0].output -> out;
        }
    }

    let mut graph = ArrayTest::new();
    graph.init(48000.0);

    // Verify the graph was created successfully
    assert_eq!(graph.sample_rate, 48000.0);
}

#[test]
fn test_array_connections() {
    graph! {
        name: ArrayConnectionTest;

        input value freq = 440.0;
        output stream out;

        nodes {
            oscs = [PolyBlepOscillator::saw(440.0, 0.6); 4];
        }

        connections {
            // Use frequency_mod (public stream input) instead of frequency (private value input)
            freq -> oscs.frequency_mod;

            oscs[0].output + oscs[1].output + oscs[2].output + oscs[3].output -> out;
        }
    }

    let mut graph = ArrayConnectionTest::new();
    graph.init(48000.0);

    // Set frequency
    graph.freq = 880.0;

    // Process a few samples
    for _ in 0..10 {
        graph.process();
    }
}
