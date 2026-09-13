use oscen::{graph, oscillators::PolyBlepOscillator};

// Test graph with ramped value inputs
graph! {
    name: RampedFilterGraph;

    // Ramped value input: 1000 frames default ramp
    input value cutoff = 1000.0 [20.0..20000.0, ramp: 1000];
    // Non-ramped value input: snaps immediately
    input value resonance = 0.707;
    // Second ramped value input for testing multiple ramps
    input value gain = 1.0 [ramp: 100];

    output stream audio_out;

    nodes {
        osc = PolyBlepOscillator::saw(440.0, 0.6);
    }

    connections {
        osc.output -> audio_out;
    }
}

#[test]
fn test_ramped_value_input_type() {
    let graph = RampedFilterGraph::new();

    // cutoff should be a ValueRampState
    assert_eq!(graph.cutoff.current, 1000.0);
    assert_eq!(graph.cutoff.target, 1000.0);
    assert!(!graph.cutoff.is_ramping());

    // resonance should be a plain f32
    assert_eq!(graph.resonance, 0.707);
}

#[test]
fn test_ramped_setter_with_default_ramp() {
    let mut graph = RampedFilterGraph::new();
    graph.init(44100.0);

    // Use default ramp (1000 frames)
    graph.set_cutoff(5000.0);

    assert!(graph.cutoff.is_ramping());
    assert_eq!(graph.cutoff.target, 5000.0);
    // current should still be at initial value before processing
    assert_eq!(graph.cutoff.current, 1000.0);

    // Process once to advance ramp
    graph.process();

    // Should have moved towards target
    assert!(graph.cutoff.current > 1000.0);
    assert!(graph.cutoff.current < 5000.0);
}

#[test]
fn test_ramped_setter_with_custom_ramp() {
    let mut graph = RampedFilterGraph::new();
    graph.init(44100.0);

    // Use custom 4-frame ramp
    graph.set_cutoff_with_ramp(100.0, 4);

    assert!(graph.cutoff.is_ramping());

    // Process 4 times
    for _ in 0..4 {
        graph.process();
    }

    // Should now be at target
    assert_eq!(graph.cutoff.current, 100.0);
    assert!(!graph.cutoff.is_ramping());
}

#[test]
fn test_ramped_setter_immediate() {
    let mut graph = RampedFilterGraph::new();
    graph.init(44100.0);

    // Set immediately
    graph.set_cutoff_immediate(8000.0);

    assert_eq!(graph.cutoff.current, 8000.0);
    assert_eq!(graph.cutoff.target, 8000.0);
    assert!(!graph.cutoff.is_ramping());
}

#[test]
fn test_non_ramped_setter() {
    let mut graph = RampedFilterGraph::new();
    graph.init(44100.0);

    // resonance has no ramp annotation, so it only has set_resonance
    graph.set_resonance(0.9);

    assert_eq!(graph.resonance, 0.9);
}

#[test]
fn test_ramped_input_used_in_connections() {
    // This is a more complex test that ensures ramped values work when
    // connected to node inputs. Since cutoff is ramped, its .current
    // value should be used when connecting to nodes.
    graph! {
        name: FilterWithRamp;

        input value freq = 440.0 [ramp: 100];
        output stream audio_out;

        nodes {
            osc = PolyBlepOscillator::saw(440.0, 0.6);
        }

        connections {
            freq -> osc.frequency_mod;
            osc.output -> audio_out;
        }
    }

    let mut graph = FilterWithRamp::new();
    graph.init(44100.0);

    // Set new frequency with ramp
    graph.set_freq(880.0);

    // Process multiple times
    let mut last_output = 0.0;
    for _ in 0..100 {
        graph.process();
        last_output = graph.audio_out;
    }

    // Should have produced audio output
    // (not zero, as oscillator was running)
    assert!(last_output.abs() > 0.0 || graph.osc.output.abs() > 0.0);

    // Frequency should have reached target
    assert_eq!(graph.freq.current, 880.0);
}

// ============================================================================
// Ramp state tests (no shared counter: every ramp is ticked each frame)
// ============================================================================

#[test]
fn test_no_ramps_active_at_start() {
    let graph = RampedFilterGraph::new();
    assert!(!graph.cutoff.is_ramping());
    assert!(!graph.gain.is_ramping());
}

#[test]
fn test_setters_start_independent_ramps() {
    let mut graph = RampedFilterGraph::new();
    graph.init(44100.0);

    graph.set_cutoff(5000.0);
    assert!(graph.cutoff.is_ramping());
    assert!(!graph.gain.is_ramping());

    graph.set_gain(0.5);
    assert!(graph.cutoff.is_ramping());
    assert!(graph.gain.is_ramping());
}

#[test]
fn test_retarget_while_ramping_keeps_ramping() {
    let mut graph = RampedFilterGraph::new();
    graph.init(44100.0);

    graph.set_cutoff(5000.0);
    assert!(graph.cutoff.is_ramping());

    graph.set_cutoff(6000.0);
    assert!(graph.cutoff.is_ramping());
    assert_eq!(graph.cutoff.target, 6000.0);
}

#[test]
fn test_ramp_completes_after_duration() {
    let mut graph = RampedFilterGraph::new();
    graph.init(44100.0);

    // Start a short ramp (4 frames)
    graph.set_cutoff_with_ramp(5000.0, 4);
    assert!(graph.cutoff.is_ramping());

    // Process until ramp completes
    for _ in 0..4 {
        graph.process();
    }

    assert!(!graph.cutoff.is_ramping());
    assert_eq!(graph.cutoff.current, 5000.0);
}

#[test]
fn test_immediate_set_cancels_ramp() {
    let mut graph = RampedFilterGraph::new();
    graph.init(44100.0);

    graph.set_cutoff(5000.0);
    assert!(graph.cutoff.is_ramping());

    graph.set_cutoff_immediate(8000.0);
    assert!(!graph.cutoff.is_ramping());
    assert_eq!(graph.cutoff.current, 8000.0);
}

#[test]
fn test_ramps_of_different_lengths_complete_independently() {
    let mut graph = RampedFilterGraph::new();
    graph.init(44100.0);

    graph.set_cutoff_with_ramp(5000.0, 10);
    graph.set_gain_with_ramp(0.5, 5);
    assert!(graph.cutoff.is_ramping());
    assert!(graph.gain.is_ramping());

    // Process 5 frames - gain ramp should complete
    for _ in 0..5 {
        graph.process();
    }
    assert!(!graph.gain.is_ramping());
    assert_eq!(graph.gain.current, 0.5);
    assert!(graph.cutoff.is_ramping());

    // Process 5 more frames - cutoff ramp should complete
    for _ in 0..5 {
        graph.process();
    }
    assert!(!graph.cutoff.is_ramping());
    assert_eq!(graph.cutoff.current, 5000.0);
}

// ----------------------------------------------------------------------------
// Direct writes to the public ramp field must not stall. The graph used to
// keep a private counter that only the generated setters maintained;
// `graph.cutoff.set_with_ramp(..)` bypassed it and the ramp never advanced.
// ----------------------------------------------------------------------------

#[test]
fn direct_set_with_ramp_is_not_stalled_per_sample() {
    let mut graph = RampedFilterGraph::new();
    graph.init(44100.0);

    graph.cutoff.set_with_ramp(5000.0, 4);
    assert!(graph.cutoff.is_ramping());
    for _ in 0..4 {
        graph.process();
    }
    assert!(!graph.cutoff.is_ramping());
    assert_eq!(graph.cutoff.current, 5000.0);
}

#[test]
fn direct_set_with_ramp_is_not_stalled_per_block() {
    let mut graph = RampedFilterGraph::new();
    graph.init(44100.0);

    graph.cutoff.set_with_ramp(5000.0, 4);
    graph.process_block(4);
    assert!(!graph.cutoff.is_ramping());
    assert_eq!(graph.cutoff.current, 5000.0);
}

#[test]
fn direct_and_generated_setters_mix_freely() {
    let mut graph = RampedFilterGraph::new();
    graph.init(44100.0);

    graph.set_gain_with_ramp(0.5, 4);
    graph.cutoff.set_with_ramp(5000.0, 8);
    for _ in 0..4 {
        graph.process();
    }
    assert!(!graph.gain.is_ramping());
    assert_eq!(graph.gain.current, 0.5);
    assert!(graph.cutoff.is_ramping());
    for _ in 0..4 {
        graph.process();
    }
    assert!(!graph.cutoff.is_ramping());
    assert_eq!(graph.cutoff.current, 5000.0);
}

#[test]
fn test_set_with_ramp_zero_frames_does_not_increment() {
    let mut graph = RampedFilterGraph::new();
    graph.init(44100.0);

    // Set with zero frames should be immediate (no ramp started)
    graph.set_cutoff_with_ramp(5000.0, 0);
    assert!(!graph.cutoff.is_ramping());
    assert_eq!(graph.cutoff.current, 5000.0);
}

#[test]
fn test_setter_is_noop_if_target_unchanged() {
    let mut graph = RampedFilterGraph::new();
    graph.init(44100.0);

    // Start a ramp
    graph.set_cutoff(5000.0);
    assert!(graph.cutoff.is_ramping());

    // Process a bit
    for _ in 0..10 {
        graph.process();
    }
    let current_after_10 = graph.cutoff.current;
    assert!(graph.cutoff.is_ramping()); // Still ramping

    // Call setter with same target - should be no-op
    graph.set_cutoff(5000.0);
    assert!(graph.cutoff.is_ramping());
    assert_eq!(graph.cutoff.current, current_after_10); // Current unchanged

    // Process more - ramp should continue normally
    for _ in 0..10 {
        graph.process();
    }
    assert!(graph.cutoff.current > current_after_10); // Still progressing
}

#[test]
fn test_setter_safe_to_call_every_frame() {
    let mut graph = RampedFilterGraph::new();
    graph.init(44100.0);

    // Use a short custom ramp (50 frames) for testing
    graph.set_cutoff_with_ramp(5000.0, 50);

    // Simulate calling setter every frame (like sync_to does)
    for _ in 0..100 {
        graph.set_cutoff_with_ramp(5000.0, 50); // Same value every frame - should be no-op
        graph.process();
    }

    // Should have completed the ramp normally (after 50 frames)
    assert_eq!(graph.cutoff.current, 5000.0);
    assert!(!graph.cutoff.is_ramping());
}
