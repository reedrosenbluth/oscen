//! Demonstrates the sample "external": load arbitrary sample data into a named,
//! realtime-swappable buffer, play it back through a `graph!`, and hot-swap the
//! source data while the graph keeps running.
//!
//! Runs headless (no audio device): it drives the graph by hand, swaps the
//! buffer mid-stream, and prints what each phase produced. Run with:
//!
//! ```sh
//! cargo run -p oscen-examples --bin sample_player_demo
//! ```

use oscen::graph::types::EventPayload;
use oscen::graph::EventInstance;
use oscen::prelude::*;
use std::sync::Arc;

graph! {
    name: SampleGraph;

    // Gate the player; >0.5 starts playback from the top.
    input trigger: event;
    // Playback speed (1.0 = original pitch).
    input rate: value = 1.0;

    output out: stream;

    nodes {
        // Reference the buffer by name. The string literal captures nothing
        // from the surrounding scope, so it is legal inside `graph!`, and it
        // resolves against the process-global sample bank at construction.
        player = SamplePlayer::from_buffer("demo");
    }

    connections {
        trigger -> player.trigger;
        rate -> player.rate;
        player.output -> out;
    }
}

/// A short, easily-recognizable buffer: a linear ramp from 0 up to `peak`.
fn ramp_buffer(frames: usize, peak: f32, source_rate: f32) -> SampleBuffer {
    let data: Vec<f32> = (0..frames)
        .map(|i| peak * i as f32 / (frames - 1) as f32)
        .collect();
    SampleBuffer::from_planar(data, 1, source_rate)
}

/// Fire a gate-on event into the graph and process `frames` samples, returning
/// everything the output produced.
fn trigger_and_collect(graph: &mut SampleGraph, frames: usize) -> Vec<f32> {
    graph.trigger.clear();
    let _ = graph.trigger.try_push(EventInstance {
        frame_offset: 0,
        payload: EventPayload::scalar(1.0),
    });

    let mut out = Vec::with_capacity(frames);
    for _ in 0..frames {
        graph.process();
        out.push(graph.get_stream_output(0).unwrap_or(0.0));
    }
    out
}

fn main() {
    let sample_rate = 48_000.0;

    // --- Control thread: load a sample and publish it under a name. ---
    // Here we synthesize one; in a real app this would be `load_wav("kick.wav")`.
    sample::global_bank().store("demo", Arc::new(ramp_buffer(8, 1.0, sample_rate)));

    let mut graph = SampleGraph::new();
    graph.init(sample_rate);

    println!("Phase 1 — play the original ramp buffer (peak 1.0):");
    let first = trigger_and_collect(&mut graph, 10);
    println!("  {first:.3?}");

    // --- Hot-swap the buffer from the "control thread" while the graph lives. ---
    // A different shape (ramp to 0.5) so the swap is obvious in the output.
    sample::global_bank().store("demo", Arc::new(ramp_buffer(8, 0.5, sample_rate)));
    println!("\nPhase 2 — after hot-swapping to a half-height ramp (peak 0.5):");
    let second = trigger_and_collect(&mut graph, 10);
    println!("  {second:.3?}");

    // --- Same data, played an octave up via the rate input. ---
    graph.set_rate(2.0);
    println!("\nPhase 3 — same buffer at rate 2.0 (octave up, every other sample):");
    let third = trigger_and_collect(&mut graph, 10);
    println!("  {third:.3?}");

    // Sanity: the peak of phase 2 should be ~half that of phase 1.
    let peak1 = first.iter().cloned().fold(0.0_f32, f32::max);
    let peak2 = second.iter().cloned().fold(0.0_f32, f32::max);
    assert!(peak1 > 0.99, "phase 1 should reach the full ramp height");
    assert!(
        (peak2 - 0.5).abs() < 0.1,
        "phase 2 should reflect the swapped-in half-height buffer"
    );
    println!("\nHot-swap verified: peak {peak1:.3} -> {peak2:.3} with no reallocation on the audio path.");

    // --- Multichannel: SamplePlayerN<N> plays a stereo buffer into Frame<N>. ---
    // (Driven directly here; inside a graph it connects to other Frame<N> nodes
    // and collapses to f32 only at the final graph output.)
    let stereo = SampleBuffer::from_interleaved(&[0.1, -0.1, 0.2, -0.2, 0.3, -0.3], 2, sample_rate);
    let mut sp = SamplePlayerN::<2>::with_slot(SampleSlot::new(Arc::new(stereo)));
    sp.set_sample_rate(sample_rate);
    sp.trigger.clear();
    let _ = sp.trigger.try_push(EventInstance {
        frame_offset: 0,
        payload: EventPayload::scalar(1.0),
    });
    sp.process_event_inputs(); // dispatch the trigger to the handler
    sp.trigger.clear(); // ...and don't let it re-fire every sample
    print!("\nPhase 4 — stereo SamplePlayerN<2> frames: ");
    for _ in 0..3 {
        sp.process();
        let Frame([l, r]) = sp.output;
        print!("[{l:.2}, {r:.2}] ");
    }
    println!();
}
