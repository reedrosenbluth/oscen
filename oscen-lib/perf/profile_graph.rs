// Run with: cargo flamegraph --bin profile_graph
// Or: cargo build --release --bin profile_graph && perf record --call-graph=dwarf ./target/release/profile_graph

use oscen::envelope::adsr::AdsrEnvelopeEndpoints;
use oscen::filters::tpt::TptFilterEndpoints;
use oscen::{
    graph, AdsrEnvelope, Delay, Graph, Oscillator, PolyBlepOscillator, PolyBlepOscillatorEndpoints,
    TptFilter,
};

// Voice subgraph: oscillator -> filter -> * envelope
graph! {
    name: Voice;

    input value frequency = 440.0;
    input event gate;
    input value cutoff = 3000.0;
    input value q = 0.707;

    output stream audio;

    node {
        osc = PolyBlepOscillator::saw(440.0, 0.6);
        filter = TptFilter::new(3000.0, 0.707);
        envelope = AdsrEnvelope::new(0.01, 0.1, 0.7, 0.2);
    }

    connection {
        frequency -> osc.frequency();
        gate -> envelope.gate();
        cutoff -> filter.cutoff();
        q -> filter.q();

        osc.output() -> filter.input();
        filter.output() * envelope.output() -> audio;
    }
}

// Polyphonic synth with 4 voices
graph! {
    name: PolySynth;

    input value cutoff = 3000.0;
    input value q = 0.707;
    input value volume = 0.8;

    output stream audio_out;

    node {
        voices = [Voice::new(sample_rate); 4];
    }

    connection {
        cutoff -> voices[0].cutoff();
        cutoff -> voices[1].cutoff();
        cutoff -> voices[2].cutoff();
        cutoff -> voices[3].cutoff();

        q -> voices[0].q();
        q -> voices[1].q();
        q -> voices[2].q();
        q -> voices[3].q();

        (voices[0].audio() + voices[1].audio() + voices[2].audio() + voices[3].audio()) * volume -> audio_out;
    }
}

fn simple_graph() -> Graph {
    let mut graph = Graph::new(44100.0);

    // Create a complex graph with multiple oscillators, filters, and effects
    let osc1 = graph.add_node(Oscillator::sine(440.0, 0.3));
    let osc2 = graph.add_node(PolyBlepOscillator::saw(450.0, 0.3));
    let osc3 = graph.add_node(Oscillator::sine(460.0, 0.3));
    let osc4 = graph.add_node(PolyBlepOscillator::saw(470.0, 0.3));
    let osc5 = graph.add_node(Oscillator::sine(480.0, 0.3));

    // Mix oscillators
    let mix1 = graph.add(osc1.output(), osc2.output());
    let mix2 = graph.add(mix1, osc3.output());
    let mix3 = graph.add(osc4.output(), osc5.output());

    // Apply filters
    let filter1 = graph.add_node(TptFilter::new(800.0, 0.5));
    let filter2 = graph.add_node(TptFilter::new(1200.0, 0.5));

    graph.connect(mix2, filter1.input());
    graph.connect(mix3, filter2.input());

    // Envelopes
    let env1 = graph.add_node(AdsrEnvelope::new(0.01, 0.1, 0.7, 0.2));
    let env2 = graph.add_node(AdsrEnvelope::new(0.02, 0.15, 0.6, 0.3));

    let filtered1 = graph.multiply(filter1.output(), env1.output());
    let filtered2 = graph.multiply(filter2.output(), env2.output());

    // Delay
    let final_mix = graph.add(filtered1, filtered2);
    let delay = graph.add_node(Delay::from_seconds(0.5, 0.3, 44100.0));

    graph.connect(final_mix, delay.input());

    graph
}

fn main() {
    use oscen::EventPayload;

    let num_samples = 441_000;
    let sample_rate = 44100.0;

    // Profile the polysynth with subgraphs
    {
        let mut synth = PolySynth::new(sample_rate);

        // Trigger some notes to simulate active voices
        synth
            .graph
            .queue_event(synth.voices[0].gate(), 0, EventPayload::scalar(1.0));
        synth
            .graph
            .queue_event(synth.voices[2].gate(), 0, EventPayload::scalar(1.0));

        // Set some frequencies
        synth.graph.set_value(synth.voices[0].frequency(), 440.0);
        synth.graph.set_value(synth.voices[2].frequency(), 554.37); // C#

        synth.graph.validate().unwrap();

        println!("=== PolySynth (4 voices with subgraphs) ===");
        println!("Processing {} samples...", num_samples);
        let start = std::time::Instant::now();

        for _ in 0..num_samples {
            synth.graph.process().unwrap();
        }

        let elapsed = start.elapsed();
        println!("Processed {} samples in {:?}", num_samples, elapsed);
        println!(
            "Samples per second: {:.2}",
            num_samples as f64 / elapsed.as_secs_f64()
        );
        println!(
            "Real-time factor: {:.2}x",
            (num_samples as f64 / 44100.0) / elapsed.as_secs_f64()
        );
        println!(
            "Microseconds per sample: {:.2}",
            elapsed.as_micros() as f64 / num_samples as f64
        );
    }

    // Profile the original test graph
    // {
    //     let mut graph = create_test_graph();
    //     graph.validate().unwrap();

    //     println!("\n=== Test Graph (5 osc + 2 filters + 2 env + delay) ===");
    //     println!("Processing {} samples...", num_samples);
    //     let start = std::time::Instant::now();

    //     for _ in 0..num_samples {
    //         graph.process().unwrap();
    //     }

    //     let elapsed = start.elapsed();
    //     println!("Processed {} samples in {:?}", num_samples, elapsed);
    //     println!(
    //         "Samples per second: {:.2}",
    //         num_samples as f64 / elapsed.as_secs_f64()
    //     );
    //     println!(
    //         "Real-time factor: {:.2}x",
    //         (num_samples as f64 / 44100.0) / elapsed.as_secs_f64()
    //     );
    //     println!(
    //         "Nanoseconds per sample: {:.2}",
    //         elapsed.as_nanos() as f64 / num_samples as f64
    //     );
    // }
}
