//! A full polyphonic FM synth graph, shared by `benches/synth_app.rs` and
//! `tests/golden_render.rs` (via `#[path]` modules).
//!
//! The shape mirrors the real example apps: the voice is the 3-operator FM
//! voice from `examples/fm-synth` (and `examples/pivot`), the top level is
//! the MIDI-driven poly architecture from `examples/pivot` /
//! `examples/electric-piano`: `MidiParser` → `VoiceAllocator<N>` →
//! `[MidiVoiceHandler; N]` → `[FmVoice; N]` → fan-in → stereo tremolo →
//! `Frame<2>` output, with the host-facing parameter set broadcast (several
//! ramped) into every voice. The helper nodes (`FmOperator`, `Crossfade`,
//! `Mixer`, `AddValue`, `Tremolo`) are copied from those examples, which
//! oscen-lib cannot depend on.

use oscen::{graph, AdsrEnvelope, Frame, Gain, Node, SampleRate, SignalProcessor, TptFilter};
use oscen::{MidiParser, MidiVoiceHandler, VoiceAllocator};
use std::f32::consts::{PI, TAU};

// --- Helper nodes (from examples/fm-synth/src/nodes/, examples/electric-piano) ---

/// FM operator: sine oscillator with phase modulation, self-feedback,
/// integrated envelope and level.
#[derive(Debug, Node)]
pub struct FmOperator {
    phase: f32,
    prev_output: f32,
    sample_rate: SampleRate,

    #[input(value)]
    pub base_freq: f32,
    #[input(value)]
    pub ratio: f32,
    #[input(stream)]
    pub phase_mod: f32,
    #[input(value)]
    pub feedback: f32,
    #[input(stream)]
    pub envelope: f32,
    #[input(value)]
    pub level: f32,
    #[output(stream)]
    pub output: f32,
}

impl FmOperator {
    pub fn new() -> Self {
        Self {
            phase: 0.0,
            prev_output: 0.0,
            sample_rate: SampleRate::default(),
            base_freq: 440.0,
            ratio: 1.0,
            phase_mod: 0.0,
            feedback: 0.0,
            envelope: 1.0,
            level: 1.0,
            output: 0.0,
        }
    }
}

impl SignalProcessor for FmOperator {
    #[inline(always)]
    fn process(&mut self) {
        let frequency = self.base_freq * self.ratio;
        let feedback_mod = self.prev_output * self.feedback;
        let total_phase_mod = self.phase_mod + feedback_mod;

        let phase_rad = (self.phase + total_phase_mod) * TAU;
        let output = phase_rad.sin() * self.envelope * self.level;
        self.output = output;
        self.prev_output = output;

        self.phase += frequency / *self.sample_rate;
        self.phase = self.phase.fract();
    }
}

/// Splits an input between two outputs based on a mix parameter.
#[derive(Debug, Node)]
pub struct Crossfade {
    #[input(stream)]
    pub input: f32,
    #[input(value)]
    pub mix: f32,
    #[output(stream)]
    pub output_a: f32,
    #[output(stream)]
    pub output_b: f32,
}

impl Crossfade {
    pub fn new() -> Self {
        Self {
            input: 0.0,
            mix: 0.0,
            output_a: 0.0,
            output_b: 0.0,
        }
    }
}

impl SignalProcessor for Crossfade {
    #[inline(always)]
    fn process(&mut self) {
        let mix = self.mix.clamp(0.0, 1.0);
        self.output_a = self.input * (1.0 - mix);
        self.output_b = self.input * mix;
    }
}

/// Adds two stream inputs.
#[derive(Debug, Node)]
pub struct Mixer {
    #[input(stream)]
    pub input_a: f32,
    #[input(stream)]
    pub input_b: f32,
    #[output(stream)]
    pub output: f32,
}

impl Mixer {
    pub fn new() -> Self {
        Self {
            input_a: 0.0,
            input_b: 0.0,
            output: 0.0,
        }
    }
}

impl SignalProcessor for Mixer {
    #[inline(always)]
    fn process(&mut self) {
        self.output = self.input_a + self.input_b;
    }
}

/// Adds a value parameter to a stream (envelope-modulates a base value).
#[derive(Debug, Node)]
pub struct AddValue {
    #[input(stream)]
    pub input: f32,
    #[input(value)]
    pub value: f32,
    #[output(stream)]
    pub output: f32,
}

impl AddValue {
    pub fn new(value: f32) -> Self {
        Self {
            input: 0.0,
            value,
            output: 0.0,
        }
    }
}

impl SignalProcessor for AddValue {
    #[inline(always)]
    fn process(&mut self) {
        self.output = self.input + self.value;
    }
}

/// Stereo tremolo: mono in, complementary L/R panning LFO, `Frame<2>` out.
#[derive(Debug, Node)]
pub struct Tremolo {
    #[input(stream)]
    pub input: f32,
    #[input(value)]
    pub rate: f32,
    #[input(value)]
    pub depth: f32,
    #[output(stream)]
    pub output: Frame<2>,

    phase: f32,
    sample_rate: SampleRate,
}

impl Tremolo {
    pub fn new() -> Self {
        Self {
            input: 0.0,
            rate: 5.0,
            depth: 0.5,
            output: Frame([0.0, 0.0]),
            phase: 0.0,
            sample_rate: SampleRate::default(),
        }
    }
}

impl SignalProcessor for Tremolo {
    #[inline(always)]
    fn process(&mut self) {
        let lfo = (self.phase * 2.0 * PI).sin();
        let pan = 0.5 + lfo * (self.depth / 3.0);
        self.output = Frame([self.input * pan, self.input * (1.0 - pan)]);
        self.phase = (self.phase + self.rate / *self.sample_rate).fract();
    }
}

// --- Voice: 3-operator FM voice with routing crossfade (examples/fm-synth) ---

graph! {
    name: FmVoice;

    input frequency: value = 440.0;
    input gate: event;

    // OP3 (top modulator)
    input op3_ratio: value = 3.0;
    input op3_level: value = 0.5;
    input op3_feedback: value = 0.0;
    input op3_attack: value = 0.01;
    input op3_decay: value = 0.1;
    input op3_sustain: value = 0.7;
    input op3_release: value = 0.3;

    // OP2 (middle modulator)
    input op2_ratio: value = 2.0;
    input op2_level: value = 0.5;
    input op2_feedback: value = 0.0;
    input op2_attack: value = 0.01;
    input op2_decay: value = 0.1;
    input op2_sustain: value = 0.7;
    input op2_release: value = 0.3;

    // OP1 (carrier)
    input op1_ratio: value = 1.0;
    input op1_attack: value = 0.01;
    input op1_decay: value = 0.2;
    input op1_sustain: value = 0.8;
    input op1_release: value = 0.5;

    // Route: 0.0 = OP3->OP2, 1.0 = OP3->OP1
    input route: value = 0.0;

    // Filter
    input filter_cutoff: value = 2000.0;
    input filter_resonance: value = 0.707;
    input filter_attack: value = 0.01;
    input filter_decay: value = 0.2;
    input filter_sustain: value = 0.5;
    input filter_release: value = 0.3;
    input filter_env_amount: value = 0.0;

    output audio_out: stream;

    nodes {
        env3 = AdsrEnvelope::new(0.01, 0.1, 0.7, 0.3);
        env2 = AdsrEnvelope::new(0.01, 0.1, 0.7, 0.3);
        env1 = AdsrEnvelope::new(0.01, 0.2, 0.8, 0.5);

        env_filter = AdsrEnvelope::new(0.01, 0.2, 0.5, 0.3);
        filter_env_gain = Gain::new(0.0);
        cutoff_mod = AddValue::new(2000.0);

        op3_osc = FmOperator::new();
        op2_osc = FmOperator::new();
        op1_osc = FmOperator::new();

        op3_route = Crossfade::new();
        op1_mod_mixer = Mixer::new();

        filter = TptFilter::new(2000.0, 0.707);
        output_gain = Gain::new(0.3);
    }

    connections {
        gate -> env3.gate;
        gate -> env2.gate;
        gate -> env1.gate;
        gate -> env_filter.gate;

        op3_attack -> env3.attack;
        op3_decay -> env3.decay;
        op3_sustain -> env3.sustain;
        op3_release -> env3.release;

        op2_attack -> env2.attack;
        op2_decay -> env2.decay;
        op2_sustain -> env2.sustain;
        op2_release -> env2.release;

        op1_attack -> env1.attack;
        op1_decay -> env1.decay;
        op1_sustain -> env1.sustain;
        op1_release -> env1.release;

        filter_attack -> env_filter.attack;
        filter_decay -> env_filter.decay;
        filter_sustain -> env_filter.sustain;
        filter_release -> env_filter.release;

        // Filter envelope modulation: env -> gain(amount) -> add(cutoff) -> filter
        env_filter.output -> filter_env_gain.input;
        filter_env_amount -> filter_env_gain.gain;
        filter_env_gain.output -> cutoff_mod.input;
        filter_cutoff -> cutoff_mod.value;
        cutoff_mod.output -> filter.cutoff;

        // OP3
        frequency -> op3_osc.base_freq;
        op3_ratio -> op3_osc.ratio;
        op3_feedback -> op3_osc.feedback;
        env3.output -> op3_osc.envelope;
        op3_level -> op3_osc.level;

        // Route crossfade: OP3 -> OP2 (a) or OP1 (b)
        op3_osc.output -> op3_route.input;
        route -> op3_route.mix;
        op3_route.output_a -> op2_osc.phase_mod;

        // OP2
        frequency -> op2_osc.base_freq;
        op2_ratio -> op2_osc.ratio;
        op2_feedback -> op2_osc.feedback;
        env2.output -> op2_osc.envelope;
        op2_level -> op2_osc.level;

        // OP2 + routed OP3 modulate OP1
        op2_osc.output -> op1_mod_mixer.input_a;
        op3_route.output_b -> op1_mod_mixer.input_b;
        op1_mod_mixer.output -> op1_osc.phase_mod;

        // OP1 (carrier) -> filter
        frequency -> op1_osc.base_freq;
        op1_ratio -> op1_osc.ratio;
        env1.output -> op1_osc.envelope;
        op1_osc.output -> filter.input;

        filter_resonance -> filter.q;

        filter.output -> output_gain.input;
        output_gain.output -> audio_out;
    }
}

// --- Top level: MIDI-driven poly synth, stamped at two voice counts ---

macro_rules! poly_synth {
    ($name:ident, $voices:tt) => {
        graph! {
            name: $name;

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

            input cutoff: value = 2000.0 [ramp: 2205];
            input resonance: value = 0.707 [ramp: 2205];
            input filter_attack: value = 0.01;
            input filter_decay: value = 0.2;
            input filter_sustain: value = 0.5;
            input filter_release: value = 0.3;
            input filter_env_amount: value = 0.0 [ramp: 2205];

            input trem_rate: value = 5.0;
            input trem_depth: value = 0.3;

            output out: stream: Frame<2>;

            nodes {
                midi_parser = MidiParser::new();
                voice_allocator = VoiceAllocator::<$voices>::new();
                voice_handlers = [MidiVoiceHandler::new(); $voices];
                voices = [FmVoice::new(); $voices];
                tremolo = Tremolo::new();
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

                cutoff -> voices.filter_cutoff;
                resonance -> voices.filter_resonance;
                filter_attack -> voices.filter_attack;
                filter_decay -> voices.filter_decay;
                filter_sustain -> voices.filter_sustain;
                filter_release -> voices.filter_release;
                filter_env_amount -> voices.filter_env_amount;

                voices.audio_out -> tremolo.input;
                trem_rate -> tremolo.rate;
                trem_depth -> tremolo.depth;

                tremolo.output -> out;
            }
        }
    };
}

poly_synth!(PolySynth8, 8);
poly_synth!(PolySynth16, 16);
