use oscen::{graph, PolyBlepOscillator, SignalProcessor};

// Test basic multiplication: osc.output * gain -> out
graph! {
    name: MultiplyToOutput;

    input value gain = 0.5;
    output stream out;

    nodes {
        osc = PolyBlepOscillator::sine(440.0, 1.0);
    }

    connections {
        osc.output * gain -> out;
    }
}

#[test]
fn test_multiply_to_output() {
    let mut graph = MultiplyToOutput::new();
    graph.init(48000.0);

    // Process a few frames
    for _ in 0..100 {
        graph.process();
    }

    // Output should be osc.output * 0.5, so bounded by [-0.5, 0.5]
    assert!(graph.out.abs() <= 0.5 + 0.001, "Output {} should be <= 0.5", graph.out);
}

// Test addition: osc1.output + osc2.output -> out
graph! {
    name: AddToOutput;

    output stream out;

    nodes {
        osc1 = PolyBlepOscillator::sine(440.0, 0.3);
        osc2 = PolyBlepOscillator::sine(880.0, 0.3);
    }

    connections {
        osc1.output + osc2.output -> out;
    }
}

#[test]
fn test_add_to_output() {
    let mut graph = AddToOutput::new();
    graph.init(48000.0);

    // Process a few frames
    for _ in 0..100 {
        graph.process();
    }

    // Output should be sum of two oscillators, bounded by [-0.6, 0.6]
    assert!(graph.out.abs() <= 0.6 + 0.001, "Output {} should be <= 0.6", graph.out);
}

// Test subtraction: osc1.output - osc2.output -> out
graph! {
    name: SubtractToOutput;

    output stream out;

    nodes {
        osc1 = PolyBlepOscillator::sine(440.0, 0.5);
        osc2 = PolyBlepOscillator::sine(440.0, 0.5);  // Same frequency
    }

    connections {
        osc1.output - osc2.output -> out;
    }
}

#[test]
fn test_subtract_to_output() {
    let mut graph = SubtractToOutput::new();
    graph.init(48000.0);

    // Process a few frames - two identical oscillators should cancel out
    for _ in 0..100 {
        graph.process();
    }

    // Same frequency oscillators started at same phase should cancel
    // (allowing small floating point error)
    assert!(graph.out.abs() < 0.001, "Output {} should be ~0 (oscillators cancel)", graph.out);
}

// Test chained expression: (osc.output * envelope) + offset -> out
graph! {
    name: ChainedExpression;

    input value envelope = 0.5;
    input value offset = 0.1;
    output stream out;

    nodes {
        osc = PolyBlepOscillator::sine(440.0, 1.0);
    }

    connections {
        osc.output * envelope + offset -> out;
    }
}

#[test]
fn test_chained_expression() {
    let mut graph = ChainedExpression::new();
    graph.init(48000.0);

    // Process a few frames
    for _ in 0..100 {
        graph.process();
    }

    // Output should be (osc * 0.5) + 0.1, so in range [-0.4, 0.6]
    assert!(graph.out >= -0.4 - 0.001 && graph.out <= 0.6 + 0.001,
            "Output {} should be in [-0.4, 0.6]", graph.out);
}

// Test three-way addition: osc1 + osc2 + osc3 -> out
graph! {
    name: ThreeWayAdd;

    output stream out;

    nodes {
        osc1 = PolyBlepOscillator::sine(440.0, 0.2);
        osc2 = PolyBlepOscillator::sine(550.0, 0.2);
        osc3 = PolyBlepOscillator::sine(660.0, 0.2);
    }

    connections {
        osc1.output + osc2.output + osc3.output -> out;
    }
}

#[test]
fn test_three_way_add() {
    let mut graph = ThreeWayAdd::new();
    graph.init(48000.0);

    // Process a few frames
    for _ in 0..100 {
        graph.process();
    }

    // Output should be sum of three oscillators, bounded by [-0.6, 0.6]
    assert!(graph.out.abs() <= 0.6 + 0.001, "Output {} should be <= 0.6", graph.out);
}

// Test multiplication with two node outputs (the original bug case)
graph! {
    name: TwoNodeMultiply;

    output stream out;

    nodes {
        osc = PolyBlepOscillator::sine(440.0, 1.0);
        lfo = PolyBlepOscillator::sine(5.0, 1.0);  // Slow LFO for amplitude modulation
    }

    connections {
        osc.output * lfo.output -> out;
    }
}

#[test]
fn test_two_node_multiply() {
    let mut graph = TwoNodeMultiply::new();
    graph.init(48000.0);

    // Process a few frames
    for _ in 0..100 {
        graph.process();
    }

    // Output should be product of two oscillators, bounded by [-1, 1]
    assert!(graph.out.abs() <= 1.0 + 0.001, "Output {} should be <= 1.0", graph.out);
    assert!(graph.out.is_finite(), "Output should be finite");
}

// Test division: osc.output / divisor -> out
graph! {
    name: DivideToOutput;

    input value divisor = 2.0;
    output stream out;

    nodes {
        osc = PolyBlepOscillator::sine(440.0, 1.0);
    }

    connections {
        osc.output / divisor -> out;
    }
}

#[test]
fn test_divide_to_output() {
    let mut graph = DivideToOutput::new();
    graph.init(48000.0);

    // Process a few frames
    for _ in 0..100 {
        graph.process();
    }

    // Output should be osc.output / 2.0, so bounded by [-0.5, 0.5]
    assert!(graph.out.abs() <= 0.5 + 0.001, "Output {} should be <= 0.5", graph.out);
}

// Test that output actually changes (not stuck at 0 due to codegen bug)
#[test]
fn test_binary_expression_produces_nonzero_output() {
    let mut graph = TwoNodeMultiply::new();
    graph.init(48000.0);

    let mut found_nonzero = false;
    for _ in 0..1000 {
        graph.process();
        if graph.out.abs() > 0.01 {
            found_nonzero = true;
            break;
        }
    }

    assert!(found_nonzero, "Binary expression should produce non-zero output at some point");
}
