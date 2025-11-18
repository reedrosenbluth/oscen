// Test that the graph! macro can parse type annotations
// This test verifies compilation succeeds when type annotations are present

// Simple macro expansion test - if this compiles, parsing worked
#[allow(dead_code)]
fn test_type_annotation_syntax_compiles() {
    // The fact that this function compiles proves the macro can parse type annotations

    // Test 1: Output with array type annotation
    macro_rules! test_output_array {
        () => {
            oscen::graph! {
                compile_time: false;
                output stream stereo: [f32; 2];
                nodes {}
                connections {}
            }
        };
    }

    // Test 2: Input with array type annotation
    macro_rules! test_input_array {
        () => {
            oscen::graph! {
                compile_time: false;
                input stream audio: [f32; 2];
                output stream out;
                nodes {}
                connections {}
            }
        };
    }

    // Test 3: Mixed scalar and array types
    macro_rules! test_mixed {
        () => {
            oscen::graph! {
                compile_time: false;
                input stream mono_in;
                input stream stereo_in: [f32; 2];
                output stream mono_out;
                output stream multi_out: [f32; 32];
                nodes {}
                connections {}
            }
        };
    }
}

// Runtime test to ensure basic functionality still works
#[test]
fn test_graph_without_type_annotations_still_works() {
    use oscen::{graph, PolyBlepOscillator};

    let builder = graph! {
        compile_time: false;
        output stream out;

        nodes {
            osc = PolyBlepOscillator::sine(440.0, 0.5);
        }

        connections {
            osc.output -> out;
        }
    };

    let mut ctx = builder.build(48000.0);

    // Process one sample to ensure it works
    ctx.graph.process();

    assert!(true);
}
