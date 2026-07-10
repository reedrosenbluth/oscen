//! Realtime-safety tests: the convolution audio path must never allocate.
//!
//! `AllocDisabler` is installed as the global allocator for this test
//! binary; any heap allocation inside an `assert_no_alloc` region aborts
//! the test. (The checks are active in debug builds, which is how `cargo
//! test` runs by default.)

use assert_no_alloc::{assert_no_alloc, AllocDisabler};
use oscen::asset::{AssetConsumer, AudioAsset};
use oscen::convolution::{
    Convolver, ConvolverConsumer, DirectConvolver, MultiConvolverEngine, PartitionedConvolver,
};
use oscen::graph;
use oscen::handoff::pair;
use oscen::spectral::FftPlan;
use oscen::SignalProcessor;

#[global_allocator]
static ALLOC: AllocDisabler = AllocDisabler;

/// Deterministic pseudo-noise in [-1, 1] (LCG; no rand dependency).
fn noise(len: usize, seed: u64) -> Vec<f32> {
    let mut state = seed
        .wrapping_mul(2862933555777941757)
        .wrapping_add(3037000493);
    (0..len)
        .map(|_| {
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            ((state >> 33) as f32 / (u32::MAX >> 1) as f32) - 1.0
        })
        .collect()
}

#[test]
fn convolver_node_process_does_not_allocate() {
    // IR long enough to engage all three tiers (head, short, long stage).
    let ir = noise(1500, 1);
    let input = noise(2048, 2);

    let mut node = Convolver::with_ir(ir);
    node.set_sample_rate(44100.0);
    node.prepare();

    // 2048 samples crosses both the 32- and 512-sample block boundaries
    // many times, including the samples where both stages fire at once.
    let sum = assert_no_alloc(|| {
        let mut sum = 0.0f32;
        for &x in &input {
            node.input = x;
            node.process();
            sum += node.output;
        }
        sum
    });
    assert!(sum.is_finite());
}

#[test]
fn convolver_structs_process_does_not_allocate() {
    let input = noise(1024, 3);

    let mut direct = DirectConvolver::new(&noise(32, 4));
    let mut partitioned = PartitionedConvolver::new(64, &noise(300, 5));

    let sum = assert_no_alloc(|| {
        let mut sum = 0.0f32;
        for &x in &input {
            sum += direct.process_sample(x) + partitioned.process_sample(x);
        }
        sum
    });
    assert!(sum.is_finite());
}

#[test]
fn handoff_take_and_retire_are_alloc_free() {
    // A payload large enough that any stray allocation would be caught.
    let (mut pubr, mut cons) = pair::<[f32; 1024]>();

    // Publish from outside the no-alloc region (publish may allocate).
    pubr.publish([0.0f32; 1024]);

    let taken = assert_no_alloc(|| {
        let taken = cons.take();
        if let Some(arc) = taken {
            cons.retire(arc);
            true
        } else {
            false
        }
    });
    assert!(taken);
}

#[test]
fn convolver_swap_is_alloc_free() {
    // Build an empty convolver with an installed consumer, then publish an
    // engine from OUTSIDE the no-alloc region (build + publish may allocate).
    let (mut publisher, consumer) = pair::<MultiConvolverEngine>();
    let mut node = Convolver::new();
    node.install_ir_consumer(consumer);
    node.set_sample_rate(44100.0);
    node.prepare();

    let ir = noise(1500, 7);
    let asset = AudioAsset::from_samples(ir, 1, 44100, 44100).unwrap();
    publisher.publish(ConvolverConsumer::<f32>::default().build(&asset).unwrap());

    // Drive enough samples to span the `take()` and the full crossfade window,
    // exercising the two-engine region and the `retire` push. None may alloc.
    let input = noise(2048, 8);
    let sum = assert_no_alloc(|| {
        let mut sum = 0.0f32;
        for &x in &input {
            node.input = x;
            node.process();
            sum += node.output;
        }
        sum
    });
    assert!(sum.is_finite());
}

#[test]
fn stereo_convolver_swap_is_alloc_free() {
    use oscen::Frame;

    // Empty stereo convolver with an installed consumer; publish a 2-channel
    // engine from OUTSIDE the no-alloc region (build + publish may allocate).
    let (mut publisher, consumer) = pair::<MultiConvolverEngine>();
    let mut node = Convolver::<Frame<2>>::new();
    node.install_ir_consumer(consumer);
    node.set_sample_rate(44100.0);
    node.prepare();

    let ir_l = noise(1500, 7);
    let ir_r = noise(1500, 11);
    let interleaved: Vec<f32> = ir_l.iter().zip(&ir_r).flat_map(|(&l, &r)| [l, r]).collect();
    let asset = AudioAsset::from_samples(interleaved, 2, 44100, 44100).unwrap();
    publisher.publish(
        ConvolverConsumer::<Frame<2>>::default()
            .build(&asset)
            .unwrap(),
    );

    // Drive enough samples to span the `take()` and the full per-channel
    // crossfade window, exercising the two-engine region and the `retire` push.
    // None may alloc.
    let input = noise(2048, 8);
    let sum = assert_no_alloc(|| {
        let mut sum = 0.0f32;
        for &x in &input {
            node.input = Frame([x, -x]);
            node.process();
            sum += node.output.0[0] + node.output.0[1];
        }
        sum
    });
    assert!(sum.is_finite());
}

#[test]
fn sample_player_swap_is_alloc_free() {
    use oscen::handoff::pair;
    use oscen::{SamplePlayer, SignalProcessor};

    let (mut publisher, consumer) = pair::<Vec<f32>>();
    let mut player = SamplePlayer::new();
    player.install_buf_consumer(consumer);

    // Publish from OUTSIDE the no-alloc region.
    publisher.publish(vec![0.25; 600]);

    // The first iteration `take`s the new buffer and `retire`s the old (empty)
    // one; neither may allocate. Looping wraps the playhead many times.
    let sum = assert_no_alloc(|| {
        let mut sum = 0.0f32;
        for _ in 0..2048 {
            player.process();
            sum += player.output;
        }
        sum
    });
    assert!(sum.is_finite());
}

#[test]
fn stereo_sample_player_swap_is_alloc_free() {
    use oscen::handoff::pair;
    use oscen::{Frame, SamplePlayer, SignalProcessor};

    let (mut publisher, consumer) = pair::<Vec<Frame<2>>>();
    let mut player = SamplePlayer::<Frame<2>>::new();
    player.install_buf_consumer(consumer);

    // Publish from OUTSIDE the no-alloc region. Distinct L/R per frame.
    publisher.publish(vec![Frame([0.25, -0.25]); 600]);

    // The first iteration `take`s the new buffer and `retire`s the old (empty)
    // one; neither may allocate. Looping wraps the playhead many times.
    let sum = assert_no_alloc(|| {
        let mut sum = 0.0f32;
        for _ in 0..2048 {
            player.process();
            sum += player.output.0[0] + player.output.0[1];
        }
        sum
    });
    assert!(sum.is_finite());
}

graph! {
    name: MidiNoAllocGraph;

    input midi_in: event;
    output stream freq_out;

    nodes {
        midi_parser = oscen::midi::MidiParser::new();
        voice_handler = oscen::midi::MidiVoiceHandler::new();
    }

    connections {
        midi_in -> midi_parser.midi_in;
        midi_parser.note_on -> voice_handler.note_on;
        midi_parser.note_off -> voice_handler.note_off;
        voice_handler.frequency -> freq_out;
    }
}

#[test]
fn midi_note_path_does_not_allocate() {
    use oscen::graph::{EventInstance, EventPayload};

    let mut graph = MidiNoAllocGraph::new();
    graph.init(44100.0);

    // Drive repeated note-on/note-off pairs through the MIDI parser and voice
    // handler; the whole note path must be heap-allocation free.
    let freq = assert_no_alloc(|| {
        let mut last = 0.0f32;
        for i in 0..1024u32 {
            let note = 60 + (i / 64 % 12) as u8;
            if i % 64 == 0 {
                let _ = graph.midi_in.try_push(EventInstance {
                    frame_offset: 0,
                    payload: EventPayload::Midi([0x90, note, 100]),
                });
            }
            if i % 64 == 32 {
                let _ = graph.midi_in.try_push(EventInstance {
                    frame_offset: 0,
                    payload: EventPayload::Midi([0x80, note, 0]),
                });
            }
            graph.process();
            last = graph.freq_out;
        }
        last
    });
    assert!(freq.is_finite());
}

#[test]
fn push_helper_does_not_allocate() {
    // The generated `push_<event_input>` helper (with the `[u8; 3]` and
    // `f32` payload conversions) is a host-facing audio-thread API; it must
    // be heap-allocation free.
    let mut graph = MidiNoAllocGraph::new();
    graph.init(44100.0);

    let freq = assert_no_alloc(|| {
        let mut last = 0.0f32;
        for i in 0..256u32 {
            let note = 60 + (i / 16 % 12) as u8;
            if i % 16 == 0 {
                let _ = graph.push_midi_in([0x90, note, 100], 0);
            }
            if i % 16 == 8 {
                let _ = graph.push_midi_in([0x80, note, 0], 0);
            }
            if i % 3 == 0 {
                let _ = graph.push_midi_in(0.5f32, 0);
            }
            graph.process();
            last = graph.freq_out;
        }
        last
    });
    assert!(freq.is_finite());
}

graph! {
    name: ParamNoAllocGraph;

    input value gain = 0.5;
    input cutoff: value = 1000.0 [20.0..20000.0, ramp: 64];
    output stream out;

    nodes {
        osc = oscen::PolyBlepOscillator::saw(440.0, 0.6);
        filter = oscen::TptFilter::new(1000.0, 0.7);
    }

    connections {
        cutoff -> filter.cutoff;
        osc.output * gain -> filter.input;
        filter.output -> out;
    }
}

#[test]
fn set_param_dispatch_does_not_allocate() {
    // The generated `set_param` / `set_param_immediate` / `get_param`
    // dispatchers are audio-thread APIs (e.g. draining a param-change queue
    // inside the callback); they must be heap-allocation free.
    let mut graph = ParamNoAllocGraph::new();
    graph.init(44100.0);

    let last = assert_no_alloc(|| {
        let mut last = 0.0f32;
        for i in 0..512u32 {
            if i % 7 == 0 {
                graph.set_param(ParamNoAllocGraphParam::Gain, (i % 10) as f32 * 0.1);
                graph.set_param(ParamNoAllocGraphParam::Cutoff, 500.0 + i as f32);
            }
            if i % 13 == 0 {
                graph.set_param_immediate(ParamNoAllocGraphParam::Cutoff, 1000.0);
            }
            let _ = graph.get_param(ParamNoAllocGraphParam::Gain);
            graph.process();
            last = graph.out;
        }
        last
    });
    assert!(last.is_finite());
}

#[test]
fn fft_plan_forward_inverse_does_not_allocate() {
    let mut plan = FftPlan::new(1024);
    let mut time = noise(1024, 6);
    let mut spectrum = plan.make_spectrum();
    let mut output = vec![0.0f32; 1024];

    assert_no_alloc(|| {
        plan.forward(&mut time, &mut spectrum);
        plan.inverse(&mut spectrum, &mut output);
    });
    assert!(output.iter().all(|y| y.is_finite()));
}
