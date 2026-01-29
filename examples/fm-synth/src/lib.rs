mod editor;
mod fm_voice;
mod nodes;
mod params;

// DSP nodes used in the graph macro
#[allow(unused_imports)]
use nodes::{AddValue, Crossfade, FmOperator, Mixer};

use fm_voice::FMVoice;
use nih_plug::prelude::*;
use oscen::graph::{EventInstance, EventPayload};
use oscen::midi::RawMidiMessage;
use oscen::prelude::*;
use params::FMParams;
use parking_lot::RwLock;
use std::sync::Arc;

// Main polyphonic FM synth with 8 voices
graph! {
    name: FMGraph;

    // MIDI input (raw MIDI bytes)
    input midi_in: event;

    // OP3 parameters
    input op3_ratio: value = 3.0;
    input op3_level: value = 0.5;
    input op3_feedback: value = 0.0;
    input op3_attack: value = 0.01;
    input op3_decay: value = 0.1;
    input op3_sustain: value = 0.7;
    input op3_release: value = 0.3;

    // OP2 parameters
    input op2_ratio: value = 2.0;
    input op2_level: value = 0.5;
    input op2_feedback: value = 0.0;
    input op2_attack: value = 0.01;
    input op2_decay: value = 0.1;
    input op2_sustain: value = 0.7;
    input op2_release: value = 0.3;

    // OP1 parameters (ratio always 1.0, no feedback)
    input op1_ratio: value = 1.0;
    input op1_attack: value = 0.01;
    input op1_decay: value = 0.2;
    input op1_sustain: value = 0.8;
    input op1_release: value = 0.5;

    // Route: blends OP3 between OP2 (0.0) and OP1 (1.0)
    input route: value = 0.0;

    // Filter parameters
    input cutoff: value = 2000.0;
    input resonance: value = 0.707;
    input filter_attack: value = 0.01;
    input filter_decay: value = 0.2;
    input filter_sustain: value = 0.5;
    input filter_release: value = 0.3;
    input filter_env_amount: value = 0.0;

    output audio_out: stream;

    nodes {
        midi_parser = MidiParser::new();
        voice_allocator = VoiceAllocator::<8>::new();
        voice_handlers = [MidiVoiceHandler::new(); 8];
        voices = [FMVoice::new(); 8];
    }

    connections {
        // MIDI parsing
        midi_in -> midi_parser.midi_in;

        // Route MIDI events through voice allocator
        midi_parser.note_on -> voice_allocator.note_on;
        midi_parser.note_off -> voice_allocator.note_off;

        // Voice allocator routes events to voice handlers
        voice_allocator.voices -> voice_handlers.note_on;
        voice_allocator.voices -> voice_handlers.note_off;

        // Voice handlers to voices
        voice_handlers.frequency -> voices.frequency;
        voice_handlers.gate -> voices.gate;

        // Broadcast OP3 parameters to all voices
        op3_ratio -> voices.op3_ratio;
        op3_level -> voices.op3_level;
        op3_feedback -> voices.op3_feedback;
        op3_attack -> voices.op3_attack;
        op3_decay -> voices.op3_decay;
        op3_sustain -> voices.op3_sustain;
        op3_release -> voices.op3_release;

        // Broadcast OP2 parameters to all voices
        op2_ratio -> voices.op2_ratio;
        op2_level -> voices.op2_level;
        op2_feedback -> voices.op2_feedback;
        op2_attack -> voices.op2_attack;
        op2_decay -> voices.op2_decay;
        op2_sustain -> voices.op2_sustain;
        op2_release -> voices.op2_release;

        // Broadcast OP1 parameters to all voices
        op1_ratio -> voices.op1_ratio;
        op1_attack -> voices.op1_attack;
        op1_decay -> voices.op1_decay;
        op1_sustain -> voices.op1_sustain;
        op1_release -> voices.op1_release;

        // Broadcast route parameter to all voices
        route -> voices.route;

        // Broadcast filter parameters to all voices
        cutoff -> voices.cutoff;
        resonance -> voices.resonance;
        filter_attack -> voices.filter_attack;
        filter_decay -> voices.filter_decay;
        filter_sustain -> voices.filter_sustain;
        filter_release -> voices.filter_release;
        filter_env_amount -> voices.filter_env_amount;

        // Mix voices
        voices.audio_out -> audio_out;
    }
}

/// Sync multiple smoothed parameters from NIH-plug params to synth graph inputs.
macro_rules! sync_params {
    ($synth:expr, $params:expr, [
        $($synth_field:ident <- $($param_path:ident).+),* $(,)?
    ]) => {
        $(
            $synth.$synth_field = $params.$($param_path).+.smoothed.next();
        )*
    };
}

pub struct FMSynth {
    params: Arc<FMParams>,
    synth: RwLock<Option<FMGraph>>,
}

impl Default for FMSynth {
    fn default() -> Self {
        Self {
            params: Arc::new(FMParams::default()),
            synth: RwLock::new(None),
        }
    }
}

impl Plugin for FMSynth {
    const NAME: &'static str = "Oscen FM";
    const VENDOR: &'static str = "Oscen";
    const URL: &'static str = "https://reed.nyc";
    const EMAIL: &'static str = "your.email@example.com";
    const VERSION: &'static str = "0.1.0";

    // Synthesizer: no input, stereo output
    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[AudioIOLayout {
        main_input_channels: None,
        main_output_channels: NonZeroU32::new(2),
        aux_input_ports: &[],
        aux_output_ports: &[],
        names: PortNames::const_default(),
    }];

    const MIDI_INPUT: MidiConfig = MidiConfig::Basic;
    const MIDI_OUTPUT: MidiConfig = MidiConfig::None;
    const SAMPLE_ACCURATE_AUTOMATION: bool = true;

    type SysExMessage = ();
    type BackgroundTask = ();

    fn params(&self) -> Arc<dyn Params> {
        self.params.clone()
    }

    fn editor(&mut self, async_executor: AsyncExecutor<Self>) -> Option<Box<dyn Editor>> {
        editor::create(self.params.clone(), async_executor)
    }

    fn initialize(
        &mut self,
        _audio_io_layout: &AudioIOLayout,
        buffer_config: &BufferConfig,
        _context: &mut impl InitContext<Self>,
    ) -> bool {
        let sample_rate = buffer_config.sample_rate;
        let mut synth = FMGraph::new();
        synth.init(sample_rate);
        *self.synth.write() = Some(synth);
        true
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        _aux: &mut AuxiliaryBuffers,
        context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        let mut synth_guard = self.synth.write();
        let synth = match synth_guard.as_mut() {
            Some(s) => s,
            None => return ProcessStatus::Normal,
        };

        // Process MIDI events
        while let Some(event) = context.next_event() {
            match event {
                NoteEvent::NoteOn {
                    note,
                    velocity,
                    timing,
                    ..
                } => {
                    let vel_byte = (velocity * 127.0).clamp(0.0, 127.0) as u8;
                    let midi_bytes = [0x90, note, vel_byte];
                    let msg = RawMidiMessage::new(&midi_bytes);
                    let event = EventInstance {
                        frame_offset: timing,
                        payload: EventPayload::Object(Arc::new(msg)),
                    };
                    let _ = synth.midi_in.try_push(event);
                }
                NoteEvent::NoteOff { note, timing, .. } => {
                    let midi_bytes = [0x80, note, 0];
                    let msg = RawMidiMessage::new(&midi_bytes);
                    let event = EventInstance {
                        frame_offset: timing,
                        payload: EventPayload::Object(Arc::new(msg)),
                    };
                    let _ = synth.midi_in.try_push(event);
                }
                _ => {}
            }
        }

        for mut channel_samples in buffer.iter_samples() {
            // Update parameters from NIH-plug's smoothed values
            sync_params!(synth, self.params, [
                // OP3
                op3_ratio <- op3.ratio,
                op3_level <- op3.level,
                op3_feedback <- op3.feedback,
                op3_attack <- op3.attack,
                op3_decay <- op3.decay,
                op3_sustain <- op3.sustain,
                op3_release <- op3.release,
                // OP2
                op2_ratio <- op2.ratio,
                op2_level <- op2.level,
                op2_feedback <- op2.feedback,
                op2_attack <- op2.attack,
                op2_decay <- op2.decay,
                op2_sustain <- op2.sustain,
                op2_release <- op2.release,
                // OP1
                op1_attack <- op1.attack,
                op1_decay <- op1.decay,
                op1_sustain <- op1.sustain,
                op1_release <- op1.release,
                // Route
                route <- route,
                // Filter
                cutoff <- filter.cutoff,
                resonance <- filter.resonance,
                filter_env_amount <- filter.env_amount,
                filter_attack <- filter.attack,
                filter_decay <- filter.decay,
                filter_sustain <- filter.sustain,
                filter_release <- filter.release,
            ]);

            // Process the graph
            synth.process();

            // Write mono output to stereo
            let output = synth.audio_out;
            for sample in channel_samples.iter_mut() {
                *sample = output;
            }
        }

        ProcessStatus::Normal
    }
}

impl ClapPlugin for FMSynth {
    const CLAP_ID: &'static str = "com.oscen.fm";
    const CLAP_DESCRIPTION: Option<&'static str> = Some("3-operator FM synthesizer");
    const CLAP_MANUAL_URL: Option<&'static str> = Some(Self::URL);
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] = &[
        ClapFeature::Instrument,
        ClapFeature::Synthesizer,
        ClapFeature::Stereo,
    ];
}

impl Vst3Plugin for FMSynth {
    const VST3_CLASS_ID: [u8; 16] = *b"OscenFMSynthPlug";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] =
        &[Vst3SubCategory::Instrument, Vst3SubCategory::Synth];
}

nih_export_clap!(FMSynth);
nih_export_vst3!(FMSynth);
