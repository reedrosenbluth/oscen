mod lp18_filter;

use lp18_filter::LP18Filter;
use nih_plug::prelude::*;
use nih_plug_egui::{create_egui_editor, egui, EguiState};
use oscen::{graph::ValueInputHandle, Graph, OutputEndpoint};
use parking_lot::RwLock;
use std::sync::Arc;

const OUTPUT_GAIN: f32 = 5.0;

pub struct TwinPeaks {
    params: Arc<TwinPeaksParams>,
    audio_context: RwLock<Option<AudioContext>>,
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

pub struct AudioContext {
    graph: Graph,
    cutoff_input_a: ValueInputHandle,
    cutoff_input_b: ValueInputHandle,
    resonance_input_a: ValueInputHandle,
    resonance_input_b: ValueInputHandle,
    output: OutputEndpoint,
    input_endpoint: ValueInputHandle,
}

impl AudioContext {
    fn new(sample_rate: f32, params: &TwinPeaksParams) -> Result<Self, &'static str> {
        let mut graph = Graph::new(sample_rate);

        let (input_signal, input_endpoint) = graph.add_audio_input();

        let filter_a = graph.add_node(LP18Filter::new(
            params.cutoff_a.value(),
            params.resonance.value(),
        ));
        let filter_b = graph.add_node(LP18Filter::new(
            params.cutoff_b.value(),
            params.resonance.value(),
        ));

        // Connect input signal to both filters
        graph.connect(input_signal.output(), filter_a.input());
        graph.connect(input_signal.output(), filter_b.input());

        // Process through twin peak filters
        let filter_diff = graph.combine(filter_a.output(), filter_b.output(), |x, y| x - y);
        let limited_output = graph.transform(filter_diff, |x| x.tanh());
        let output = graph.transform(limited_output, |x| x * OUTPUT_GAIN);

        // Connect graph
        if graph
            .insert_value_input(filter_a.cutoff(), params.cutoff_a.value())
            .is_none()
        {
            return Err("Failed to insert filter A cutoff input");
        }

        if graph
            .insert_value_input(filter_b.cutoff(), params.cutoff_b.value())
            .is_none()
        {
            return Err("Failed to insert filter B cutoff input");
        }

        if graph
            .insert_value_input(filter_a.resonance(), params.resonance.value())
            .is_none()
        {
            return Err("Failed to insert filter A Q input");
        }

        if graph
            .insert_value_input(filter_b.resonance(), params.resonance.value())
            .is_none()
        {
            return Err("Failed to insert filter B Q input");
        }

        Ok(Self {
            graph,
            cutoff_input_a: filter_a.cutoff(),
            cutoff_input_b: filter_b.cutoff(),
            resonance_input_a: filter_a.resonance(),
            resonance_input_b: filter_b.resonance(),
            output,
            input_endpoint,
        })
    }

    fn update_params(&mut self, params: &TwinPeaksParams) {
        // Using immediate updates since NIH-plug already handles parameter smoothing
        self.graph
            .set_value(self.cutoff_input_a, params.cutoff_a.smoothed.next());
        self.graph
            .set_value(self.cutoff_input_b, params.cutoff_b.smoothed.next());
        self.graph
            .set_value(self.resonance_input_a, params.resonance.smoothed.next());
        self.graph
            .set_value(self.resonance_input_b, params.resonance.smoothed.next());
    }
}

impl Default for TwinPeaks {
    fn default() -> Self {
        Self {
            params: Arc::new(TwinPeaksParams::default()),
            audio_context: RwLock::new(None),
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

        match AudioContext::new(sample_rate, &self.params) {
            Ok(audio_context) => {
                *self.audio_context.write() = Some(audio_context);
                true
            }
            Err(_) => false,
        }
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        _aux: &mut AuxiliaryBuffers,
        _context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        let mut audio_context_guard = self.audio_context.write();
        if let Some(audio_context) = audio_context_guard.as_mut() {
            for mut channel_samples in buffer.iter_samples() {
                // Update parameters
                audio_context.update_params(&self.params);

                // Get input sample from first channel
                let input_sample = channel_samples.iter_mut().next().map(|s| *s).unwrap_or(0.0);

                // Feed input to the graph
                audio_context
                    .graph
                    .set_value(audio_context.input_endpoint, input_sample);

                // Process the graph
                let _ = audio_context.graph.process();

                // Write output to all channels
                if let Some(output_value) = audio_context.graph.get_value(&audio_context.output) {
                    for sample in channel_samples {
                        *sample = output_value;
                    }
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
