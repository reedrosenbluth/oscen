mod midi_input;

use std::cell::Cell;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use oscen::prelude::*;

use fm_synth::fm_voice::FMVoice;
#[allow(unused_imports)]
use fm_synth::nodes::{AddValue, Crossfade, FmOperator, Mixer};
use fm_synth::waveform;
use midi_input::MidiConnection;

slint::include_modules!();

#[derive(Clone, Copy, Debug)]
enum ParamChange {
    Op3Ratio(f32),
    Op3Level(f32),
    Op3Feedback(f32),
    Op3Attack(f32),
    Op3Decay(f32),
    Op3Sustain(f32),
    Op3Release(f32),
    Op2Ratio(f32),
    Op2Level(f32),
    Op2Feedback(f32),
    Op2Attack(f32),
    Op2Decay(f32),
    Op2Sustain(f32),
    Op2Release(f32),
    Op1Attack(f32),
    Op1Decay(f32),
    Op1Sustain(f32),
    Op1Release(f32),
    Route(f32),
    FilterCutoff(f32),
    FilterResonance(f32),
    FilterAttack(f32),
    FilterDecay(f32),
    FilterSustain(f32),
    FilterRelease(f32),
    FilterEnvAmount(f32),
}

graph! {
    name: FMStandaloneGraph;

    input midi_in: event;

    input op3_ratio: value = 3.0;
    input op3_level: value = 0.5 [ramp: 2205];
    input op3_feedback: value = 0.0 [ramp: 2205];
    input op3_attack: value = 0.01;
    input op3_decay: value = 0.1;
    input op3_sustain: value = 0.7;
    input op3_release: value = 0.3;

    input op2_ratio: value = 2.0;
    input op2_level: value = 0.5 [ramp: 2205];
    input op2_feedback: value = 0.0 [ramp: 2205];
    input op2_attack: value = 0.01;
    input op2_decay: value = 0.1;
    input op2_sustain: value = 0.7;
    input op2_release: value = 0.3;

    input op1_ratio: value = 1.0;
    input op1_attack: value = 0.01;
    input op1_decay: value = 0.2;
    input op1_sustain: value = 0.8;
    input op1_release: value = 0.5;

    input route: value = 0.0 [ramp: 2205];

    input filter_cutoff: value = 2000.0 [ramp: 2205];
    input filter_resonance: value = 0.707 [ramp: 2205];
    input filter_attack: value = 0.01;
    input filter_decay: value = 0.2;
    input filter_sustain: value = 0.5;
    input filter_release: value = 0.3;
    input filter_env_amount: value = 0.0 [ramp: 2205];

    output audio_out: stream;

    nodes {
        midi_parser = MidiParser::new();
        voice_allocator = VoiceAllocator::<8>::new();
        voice_handlers = [MidiVoiceHandler::new(); 8];
        voices = [FMVoice::new(); 8];
    }

    connections {
        midi_in -> midi_parser.midi_in;
        midi_parser.note_on -> voice_allocator.note_on;
        midi_parser.note_off -> voice_allocator.note_off;
        voice_allocator.voices -> voice_handlers.note_on;
        voice_allocator.voices -> voice_handlers.note_off;
        voice_handlers.frequency -> voices.frequency;
        voice_handlers.gate -> voices.gate;

        op3_ratio -> voices.op3_ratio;
        op3_level -> voices.op3_level;
        op3_feedback -> voices.op3_feedback;
        op3_attack -> voices.op3_attack;
        op3_decay -> voices.op3_decay;
        op3_sustain -> voices.op3_sustain;
        op3_release -> voices.op3_release;

        op2_ratio -> voices.op2_ratio;
        op2_level -> voices.op2_level;
        op2_feedback -> voices.op2_feedback;
        op2_attack -> voices.op2_attack;
        op2_decay -> voices.op2_decay;
        op2_sustain -> voices.op2_sustain;
        op2_release -> voices.op2_release;

        op1_ratio -> voices.op1_ratio;
        op1_attack -> voices.op1_attack;
        op1_decay -> voices.op1_decay;
        op1_sustain -> voices.op1_sustain;
        op1_release -> voices.op1_release;

        route -> voices.route;

        filter_cutoff -> voices.filter_cutoff;
        filter_resonance -> voices.filter_resonance;
        filter_attack -> voices.filter_attack;
        filter_decay -> voices.filter_decay;
        filter_sustain -> voices.filter_sustain;
        filter_release -> voices.filter_release;
        filter_env_amount -> voices.filter_env_amount;

        voices.audio_out -> audio_out;
    }
}

struct AudioContext {
    synth: FMStandaloneGraph,
    channels: usize,
}

fn audio_callback(
    data: &mut [f32],
    context: &mut AudioContext,
    param_rx: &Receiver<ParamChange>,
    midi_rx: &Receiver<midi_input::RawMidiBytes>,
) {
    use oscen::graph::{EventInstance, EventPayload};
    use oscen::midi::RawMidiMessage;

    while let Ok(raw_midi) = midi_rx.try_recv() {
        let msg = RawMidiMessage::new(&raw_midi.bytes);
        let event = EventInstance {
            frame_offset: 0,
            payload: EventPayload::Object(std::sync::Arc::new(msg)),
        };
        let _ = context.synth.midi_in.try_push(event);
    }

    while let Ok(change) = param_rx.try_recv() {
        match change {
            ParamChange::Op3Ratio(v) => context.synth.op3_ratio = v,
            ParamChange::Op3Level(v) => context.synth.set_op3_level(v),
            ParamChange::Op3Feedback(v) => context.synth.set_op3_feedback(v),
            ParamChange::Op3Attack(v) => context.synth.op3_attack = v,
            ParamChange::Op3Decay(v) => context.synth.op3_decay = v,
            ParamChange::Op3Sustain(v) => context.synth.op3_sustain = v,
            ParamChange::Op3Release(v) => context.synth.op3_release = v,
            ParamChange::Op2Ratio(v) => context.synth.op2_ratio = v,
            ParamChange::Op2Level(v) => context.synth.set_op2_level(v),
            ParamChange::Op2Feedback(v) => context.synth.set_op2_feedback(v),
            ParamChange::Op2Attack(v) => context.synth.op2_attack = v,
            ParamChange::Op2Decay(v) => context.synth.op2_decay = v,
            ParamChange::Op2Sustain(v) => context.synth.op2_sustain = v,
            ParamChange::Op2Release(v) => context.synth.op2_release = v,
            ParamChange::Op1Attack(v) => context.synth.op1_attack = v,
            ParamChange::Op1Decay(v) => context.synth.op1_decay = v,
            ParamChange::Op1Sustain(v) => context.synth.op1_sustain = v,
            ParamChange::Op1Release(v) => context.synth.op1_release = v,
            ParamChange::Route(v) => context.synth.set_route(v),
            ParamChange::FilterCutoff(v) => context.synth.set_filter_cutoff(v),
            ParamChange::FilterResonance(v) => context.synth.set_filter_resonance(v),
            ParamChange::FilterAttack(v) => context.synth.filter_attack = v,
            ParamChange::FilterDecay(v) => context.synth.filter_decay = v,
            ParamChange::FilterSustain(v) => context.synth.filter_sustain = v,
            ParamChange::FilterRelease(v) => context.synth.filter_release = v,
            ParamChange::FilterEnvAmount(v) => context.synth.set_filter_env_amount(v),
        }
    }

    // Block-based processing: process all frames at once
    let frames = data.len() / context.channels;
    let frames = frames.min(FMStandaloneGraph::MAX_BLOCK_SIZE);
    context.synth.process_block(frames);

    // Copy from output block buffer to interleaved audio output
    for (i, frame) in data.chunks_mut(context.channels).enumerate() {
        let mono = context.synth.audio_out_block[i];
        if context.channels >= 2 {
            frame[0] = mono;
            frame[1] = mono;
            for sample in frame.iter_mut().skip(2) {
                *sample = 0.0;
            }
        } else if context.channels == 1 {
            frame[0] = mono;
        }
    }
}

thread_local! {
    static LAST_RENDER: Cell<Option<Instant>> = const { Cell::new(None) };
}

fn render_waveform(ui: &SynthWindow) {
    let skip = LAST_RENDER.with(|cell| {
        cell.get()
            .map(|t| t.elapsed() < Duration::from_millis(33))
            .unwrap_or(false)
    });
    if skip {
        return;
    }
    LAST_RENDER.with(|cell| cell.set(Some(Instant::now())));

    let params = waveform::FmWaveformParams {
        op3_ratio: ui.get_op3_ratio(),
        op3_level: ui.get_op3_level(),
        op3_feedback: ui.get_op3_feedback(),
        op2_ratio: ui.get_op2_ratio(),
        op2_level: ui.get_op2_level(),
        op2_feedback: ui.get_op2_feedback(),
        route: ui.get_route(),
    };
    let image = waveform::render_image(&params);
    ui.set_waveform_image(image);
}

fn main() -> Result<()> {
    // Force the GPU renderer for smooth anti-aliased UI elements
    std::env::set_var("SLINT_BACKEND", "winit-femtovg");

    let (param_tx, param_rx) = mpsc::channel();
    let (midi_tx, midi_rx) = mpsc::channel();
    let _midi_connection = MidiConnection::new(midi_tx)?;

    thread::Builder::new()
        .name("audio".into())
        .spawn(move || {
            let host = cpal::default_host();
            let device = match host.default_output_device() {
                Some(d) => d,
                None => {
                    eprintln!("No output device available");
                    return;
                }
            };

            let default_config = match device.default_output_config() {
                Ok(c) => c,
                Err(err) => {
                    eprintln!("Failed to get output config: {}", err);
                    return;
                }
            };

            let config = cpal::StreamConfig {
                channels: default_config.channels(),
                sample_rate: default_config.sample_rate(),
                buffer_size: cpal::BufferSize::Fixed(512),
            };

            let sample_rate = config.sample_rate.0 as f32;
            let mut synth = FMStandaloneGraph::new();
            synth.init(sample_rate);
            let mut context = AudioContext {
                synth,
                channels: config.channels as usize,
            };

            let stream = match device.build_output_stream(
                &config,
                move |data: &mut [f32], _| {
                    audio_callback(data, &mut context, &param_rx, &midi_rx);
                },
                |err| eprintln!("Audio stream error: {}", err),
                None,
            ) {
                Ok(s) => s,
                Err(err) => {
                    eprintln!("Failed to build output stream: {}", err);
                    return;
                }
            };

            if let Err(err) = stream.play() {
                eprintln!("Failed to start audio stream: {}", err);
                return;
            }

            loop {
                thread::sleep(Duration::from_millis(100));
            }
        })
        .context("failed to spawn audio thread")?;

    run_ui(param_tx)?;
    Ok(())
}

macro_rules! bind_param {
    ($ui:expr, $tx:expr, $ui_weak:expr, $callback:ident, $variant:ident) => {{
        let tx = $tx.clone();
        let ui_weak = $ui_weak.clone();
        $ui.$callback(move |value| {
            let _ = tx.send(ParamChange::$variant(value));
            if let Some(ui) = ui_weak.upgrade() {
                render_waveform(&ui);
            }
        });
    }};
}

fn run_ui(tx: Sender<ParamChange>) -> Result<()> {
    let ui = SynthWindow::new()?;
    let ui_weak = ui.as_weak();

    bind_param!(ui, tx, ui_weak, on_op3_ratio_edited, Op3Ratio);
    bind_param!(ui, tx, ui_weak, on_op3_level_edited, Op3Level);
    bind_param!(ui, tx, ui_weak, on_op3_feedback_edited, Op3Feedback);
    bind_param!(ui, tx, ui_weak, on_op3_attack_edited, Op3Attack);
    bind_param!(ui, tx, ui_weak, on_op3_decay_edited, Op3Decay);
    bind_param!(ui, tx, ui_weak, on_op3_sustain_edited, Op3Sustain);
    bind_param!(ui, tx, ui_weak, on_op3_release_edited, Op3Release);

    bind_param!(ui, tx, ui_weak, on_op2_ratio_edited, Op2Ratio);
    bind_param!(ui, tx, ui_weak, on_op2_level_edited, Op2Level);
    bind_param!(ui, tx, ui_weak, on_op2_feedback_edited, Op2Feedback);
    bind_param!(ui, tx, ui_weak, on_op2_attack_edited, Op2Attack);
    bind_param!(ui, tx, ui_weak, on_op2_decay_edited, Op2Decay);
    bind_param!(ui, tx, ui_weak, on_op2_sustain_edited, Op2Sustain);
    bind_param!(ui, tx, ui_weak, on_op2_release_edited, Op2Release);

    bind_param!(ui, tx, ui_weak, on_op1_attack_edited, Op1Attack);
    bind_param!(ui, tx, ui_weak, on_op1_decay_edited, Op1Decay);
    bind_param!(ui, tx, ui_weak, on_op1_sustain_edited, Op1Sustain);
    bind_param!(ui, tx, ui_weak, on_op1_release_edited, Op1Release);

    bind_param!(ui, tx, ui_weak, on_route_edited, Route);

    bind_param!(ui, tx, ui_weak, on_filter_cutoff_edited, FilterCutoff);
    bind_param!(ui, tx, ui_weak, on_filter_resonance_edited, FilterResonance);
    bind_param!(ui, tx, ui_weak, on_filter_attack_edited, FilterAttack);
    bind_param!(ui, tx, ui_weak, on_filter_decay_edited, FilterDecay);
    bind_param!(ui, tx, ui_weak, on_filter_sustain_edited, FilterSustain);
    bind_param!(ui, tx, ui_weak, on_filter_release_edited, FilterRelease);
    bind_param!(ui, tx, ui_weak, on_filter_env_amount_edited, FilterEnvAmount);

    render_waveform(&ui);

    ui.on_knob_drag_ended({
        let ui_weak = ui_weak.clone();
        move || {
            if let Some(ui) = ui_weak.upgrade() {
                LAST_RENDER.with(|cell| cell.set(None));
                render_waveform(&ui);
            }
        }
    });

    ui.run()?;
    Ok(())
}
