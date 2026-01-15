use oscen::graph;
use oscen::oscillators::PolyBlepOscillator;
use oscen::filters::TptFilter;
use oscen::SignalProcessor;

#[test]
fn test_static_graph_compilation() {
    graph! {
        name: StaticGraph;

        input value freq = 440.0;
        output stream out;

        nodes {
            osc = PolyBlepOscillator::saw(440.0, 0.6);
            filter = TptFilter::new(1000.0, 0.707);
        }

        connections {
            // Use frequency_mod (public stream input) instead of frequency (private value input)
            freq -> osc.frequency_mod;
            osc.output -> filter.input;
            filter.output -> out;
        }
    }

    let mut graph = StaticGraph::new();
    graph.init(48000.0);

    // Test processing
    graph.process();

    // Check output (should be 0.0 initially or some value)
    // Since we can't easily predict the exact value without running many samples,
    // we just ensure it compiles and runs.
    let _out = graph.out;
}
