mod lp18_filter;

use lp18_filter::LP18Filter;
use nih_plug::prelude::*;
use nih_plug_egui::{create_egui_editor, egui, EguiState};
use oscen::prelude::*;
use parking_lot::RwLock;
use std::sync::Arc;

const OUTPUT_GAIN: f32 = 5.0;

// Static graph definition for the twin peaks filter
graph! {
    name: TwinPeaksGraph;

    // Audio input
    input audio_in: stream;

    // Parameters
    input cutoff_a: value = 1000.0;
    input cutoff_b: value = 1900.0;
    input resonance: value = 0.54;

    // Audio output (raw filter difference, post-processing done outside graph)
    output audio_out: stream;

    nodes {
        filter_a = LP18Filter::new(1000.0, 0.54);
        filter_b = LP18Filter::new(1900.0, 0.54);
    }

    connections {
        // Feed input to both filters
        audio_in -> filter_a.input;
        audio_in -> filter_b.input;

        // Connect parameters
        cutoff_a -> filter_a.cutoff;
        cutoff_b -> filter_b.cutoff;
        resonance -> filter_a.resonance;
        resonance -> filter_b.resonance;

        // Twin peaks: difference of two filters
        filter_a.output - filter_b.output -> audio_out;
    }
}

pub struct TwinPeaks {
    params: Arc<TwinPeaksParams>,
    synth: RwLock<Option<TwinPeaksGraph>>,
}

#[derive(Params)]
pub struct TwinPeaksParams {
    #[persist = "editor-state"]
    editor_state: Arc<EguiState>,

    #[id = "cutoff_a"]
    pub cutoff_a: FloatParam,

    #[id = "cutoff_b"]
    pub cutoff_b: FloatParam,

    #[id = "resonance"]
    pub resonance: FloatParam,
}

impl Default for TwinPeaksParams {
    fn default() -> Self {
        Self {
            editor_state: EguiState::from_size(200, 220),

            cutoff_a: FloatParam::new(
                "Cutoff A",
                1000.0,
                FloatRange::Skewed {
                    min: 20.0,
                    max: 14500.0,
                    factor: FloatRange::skew_factor(-2.0),
                },
            )
            .with_smoother(SmoothingStyle::Linear(50.0))
            .with_unit(" Hz")
            .with_value_to_string(formatters::v2s_f32_rounded(0)),

            cutoff_b: FloatParam::new(
                "Cutoff B",
                1900.0,
                FloatRange::Skewed {
                    min: 20.0,
                    max: 14500.0,
                    factor: FloatRange::skew_factor(-2.0),
                },
            )
            .with_smoother(SmoothingStyle::Linear(50.0))
            .with_unit(" Hz")
            .with_value_to_string(formatters::v2s_f32_rounded(0)),

            resonance: FloatParam::new(
                "Resonance",
                0.54,
                FloatRange::Linear {
                    min: 0.4,
                    max: 0.99,
                },
            )
            .with_smoother(SmoothingStyle::Linear(50.0))
            .with_value_to_string(formatters::v2s_f32_rounded(3)),
        }
    }
}

impl Default for TwinPeaks {
    fn default() -> Self {
        Self {
            params: Arc::new(TwinPeaksParams::default()),
            synth: RwLock::new(None),
        }
    }
}

impl Plugin for TwinPeaks {
    const NAME: &'static str = "Twin Peak Filter";
    const VENDOR: &'static str = "Oscen";
    const URL: &'static str = "https://reed.nyc";
    const EMAIL: &'static str = "your.email@example.com";
    const VERSION: &'static str = "0.1.0";

    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[AudioIOLayout {
        main_input_channels: NonZeroU32::new(1),  // Mono input
        main_output_channels: NonZeroU32::new(1), // Mono output
        aux_input_ports: &[],
        aux_output_ports: &[],
        names: PortNames::const_default(),
    }];

    const MIDI_INPUT: MidiConfig = MidiConfig::None;
    const MIDI_OUTPUT: MidiConfig = MidiConfig::None;
    const SAMPLE_ACCURATE_AUTOMATION: bool = true;

    type SysExMessage = ();
    type BackgroundTask = ();

    fn params(&self) -> Arc<dyn Params> {
        self.params.clone()
    }

    fn editor(&mut self, _async_executor: AsyncExecutor<Self>) -> Option<Box<dyn Editor>> {
        let params = self.params.clone();
        create_egui_editor(
            self.params.editor_state.clone(),
            (),
            |_, _| {},
            move |egui_ctx, setter, _state| {
                egui::CentralPanel::default().show(egui_ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.group(|ui| {
                            ui.vertical(|ui| {
                                ui.heading("Filter");
                                ui.add_space(20.0);

                                ui.label("Filter A Cutoff");
                                ui.add(nih_plug_egui::widgets::ParamSlider::for_param(
                                    &params.cutoff_a,
                                    setter,
                                ));
                                ui.add_space(10.0);

                                ui.label("Filter B Cutoff");
                                ui.add(nih_plug_egui::widgets::ParamSlider::for_param(
                                    &params.cutoff_b,
                                    setter,
                                ));
                                ui.add_space(10.0);

                                ui.label("Resonance (both filters)");
                                ui.add(nih_plug_egui::widgets::ParamSlider::for_param(
                                    &params.resonance,
                                    setter,
                                ));
                            });
                        });
                    });
                });
            },
        )
    }

    fn initialize(
        &mut self,
        _audio_io_layout: &AudioIOLayout,
        buffer_config: &BufferConfig,
        _context: &mut impl InitContext<Self>,
    ) -> bool {
        let sample_rate = buffer_config.sample_rate;
        let mut synth = TwinPeaksGraph::new();
        synth.init(sample_rate);
        *self.synth.write() = Some(synth);
        true
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        _aux: &mut AuxiliaryBuffers,
        _context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        let mut synth_guard = self.synth.write();
        if let Some(synth) = synth_guard.as_mut() {
            for mut channel_samples in buffer.iter_samples() {
                // Update parameters from NIH-plug's smoothed values
                synth.cutoff_a = self.params.cutoff_a.smoothed.next();
                synth.cutoff_b = self.params.cutoff_b.smoothed.next();
                synth.resonance = self.params.resonance.smoothed.next();

                // Get input sample from first channel
                let input_sample = channel_samples.iter_mut().next().map(|s| *s).unwrap_or(0.0);

                // Feed input to the graph
                synth.audio_in = input_sample;

                // Process the graph
                synth.process();

                // Apply tanh soft clipping and gain, then write to all channels
                let output = synth.audio_out.tanh() * OUTPUT_GAIN;
                for sample in channel_samples {
                    *sample = output;
                }
            }
        }

        ProcessStatus::Normal
    }
}

impl ClapPlugin for TwinPeaks {
    const CLAP_ID: &'static str = "com.oscen.twin-peak-nih";
    const CLAP_DESCRIPTION: Option<&'static str> = Some("Twin Peak Filter");
    const CLAP_MANUAL_URL: Option<&'static str> = Some(Self::URL);
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] = &[ClapFeature::AudioEffect, ClapFeature::Filter];
}

impl Vst3Plugin for TwinPeaks {
    const VST3_CLASS_ID: [u8; 16] = *b"TwinPeaksNIHPlug";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] =
        &[Vst3SubCategory::Fx, Vst3SubCategory::Filter];
}

nih_export_clap!(TwinPeaks);
nih_export_vst3!(TwinPeaks);
