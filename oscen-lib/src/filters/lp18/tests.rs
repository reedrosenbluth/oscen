use super::*;
use crate::{Graph, Oscillator};

#[test]
fn test_lp18_rolloff_slope() {
    // Test the 18dB/octave slope characteristic
    let sample_rate = 44100.0;
    let cutoff = 1000.0;

    // Create measurements at cutoff, 2x cutoff, and 4x cutoff
    let measurements = vec![
        (cutoff, 0.0),         // Reference point (normalized to 0dB)
        (cutoff * 2.0, -18.0), // 1 octave above: should be -18dB
        (cutoff * 4.0, -36.0), // 2 octaves above: should be -36dB
    ];

    let mut results = Vec::new();

    // For each test frequency
    for (freq, _) in &measurements {
        // Create a new graph for each test to avoid needing to remove nodes
        let mut graph = Graph::new(sample_rate);
        let osc = graph.add_node(Oscillator::sine(*freq, 1.0));
        let filter = graph.add_node(LP18Filter::new(cutoff, 0.0));
        graph.connect(osc.output(), filter.audio_in());

        // Stabilize and measure
        for _ in 0..10000 {
            graph.process();
        }

        let mut peak = 0.0f32;
        for _ in 0..2000 {
            graph.process();
            if let Some(value) = graph.get_value(&filter.audio_out()) {
                peak = peak.max(value.abs());
            }
        }

        results.push(peak);
    }

    // Convert to dB and normalize to first measurement
    let reference_db = 20.0 * results[0].log10();
    let db_values: Vec<f32> = results
        .iter()
        .map(|&v| 20.0 * v.log10() - reference_db)
        .collect();

    println!("LP18 Filter frequency response:");
    for i in 0..measurements.len() {
        println!("  At {}Hz: {}dB", measurements[i].0, db_values[i]);
    }

    // Check each measurement point with reasonable tolerance
    for i in 1..measurements.len() {
        let (_, expected_db) = measurements[i];
        let actual_db = db_values[i];
        assert!(
            (actual_db - expected_db).abs() < 3.0,
            "At {}Hz: expected {}dB, got {}dB",
            measurements[i].0,
            expected_db,
            actual_db
        );
    }
}