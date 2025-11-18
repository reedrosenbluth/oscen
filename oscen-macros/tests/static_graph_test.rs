use oscen::graph;
use oscen::oscillators::PolyBlepOscillator;
use oscen::filters::TptFilter;

#[test]
fn test_static_graph_compilation() {
    graph! {
        name: StaticGraph;
        compile_time: true;

        input value freq = 440.0;
        output stream out;

        node osc = PolyBlepOscillator::saw(440.0, 0.6);
        node filter = TptFilter::new(1000.0, 0.707);

        connection freq -> osc.frequency();
        connection osc.output() -> filter.input();
        connection filter.output() -> out;
    }

    let mut graph = StaticGraph::new(48000.0);
    
    // Test processing
    graph.process();
    
    // Check output (should be 0.0 initially or some value)
    // Since we can't easily predict the exact value without running many samples,
    // we just ensure it compiles and runs.
    let _out = graph.out;
}
