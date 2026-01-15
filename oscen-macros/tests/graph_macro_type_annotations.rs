// Test that the graph! macro can parse type annotations
// This test verifies compilation succeeds when type annotations are present

use oscen::{graph, PolyBlepOscillator, SignalProcessor};

// Test 1: Output with array type annotation
graph! {
    name: TestOutputArray;
    output stream stereo: [f32; 2];
    nodes {}
    connections {}
}

// Test 2: Input with array type annotation
graph! {
    name: TestInputArray;
    input stream audio: [f32; 2];
    output stream out;
    nodes {}
    connections {}
}

// Test 3: Mixed scalar and array types
graph! {
    name: TestMixed;
    input stream mono_in;
    input stream stereo_in: [f32; 2];
    output stream mono_out;
    output stream multi_out: [f32; 32];
    nodes {}
    connections {}
}

// Runtime test to ensure basic functionality still works
#[test]
fn test_graph_without_type_annotations_still_works() {
    graph! {
        name: BasicGraph;
        output stream out;

        nodes {
            osc = PolyBlepOscillator::sine(440.0, 0.5);
        }

        connections {
            osc.output -> out;
        }
    }

    let mut graph = BasicGraph::new();
    graph.init(48000.0);

    // Process one sample to ensure it works
    graph.process();

    assert!(true);
}

#[test]
fn test_type_annotations_compile() {
    // These should compile without errors
    let mut graph1 = TestOutputArray::new();
    graph1.init(48000.0);

    let mut graph2 = TestInputArray::new();
    graph2.init(48000.0);

    let mut graph3 = TestMixed::new();
    graph3.init(48000.0);
}
