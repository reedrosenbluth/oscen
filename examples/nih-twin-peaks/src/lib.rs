use nih_plug::prelude::*;
use nih_plug_egui::{create_egui_editor, egui, EguiState};
use oscen::{Graph, LP18Filter, Oscillator, OutputEndpoint, ValueKey};
use parking_lot::RwLock;
use std::sync::Arc;

pub struct TwinPeaks {
    params: Arc<TwinPeaksParams>,
    audio_context: RwLock<Option<AudioContext>>,
}

#[derive(Params)]
pub struct TwinPeaksParams {
    #[persist = "editor-state"]
    editor_state: Arc<EguiState>,

    #[id = "frequency"]
    pub frequency: FloatParam,

    #[id = "cutoff_a"]
    pub cutoff_frequency_a: FloatParam,

    #[id = "cutoff_b"]
    pub cutoff_frequency_b: FloatParam,

    #[id = "q_factor"]
    pub q_factor: FloatParam,
}

impl Default for TwinPeaksParams {
    fn default() -> Self {
        Self {
            editor_state: EguiState::from_size(370, 220),

            frequency: FloatParam::new(
                "Frequency",
                3.0,
                FloatRange::Linear {
                    min: 0.1,
                    max: 10.0,
                },
            )
            .with_smoother(SmoothingStyle::Linear(50.0))
            .with_unit(" Hz")
            .with_value_to_string(formatters::v2s_f32_rounded(1)),

            cutoff_frequency_a: FloatParam::new(
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

            cutoff_frequency_b: FloatParam::new(
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

            q_factor: FloatParam::new(
                "Q Factor",
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
    oscillator_freq_input: ValueKey,
    cutoff_freq_input_a: ValueKey,
    cutoff_freq_input_b: ValueKey,
    q_input_a: ValueKey,
    q_input_b: ValueKey,
    output: OutputEndpoint,
}

impl AudioContext {
    fn new(sample_rate: f32, params: &TwinPeaksParams) -> Result<Self, &'static str> {
        let mut graph = Graph::new(sample_rate);

        let pulse_osc = graph.add_node(Oscillator::new(params.frequency.value(), 1.0, |p| {
            if p < 0.001 {
                1.0
            } else {
                0.0
            }
        }));

        let filter_a = graph.add_node(LP18Filter::new(
            params.cutoff_frequency_a.value(),
            params.q_factor.value(),
        ));
        let filter_b = graph.add_node(LP18Filter::new(
            params.cutoff_frequency_b.value(),
            params.q_factor.value(),
        ));

        let sequencer = graph.transform(pulse_osc.output(), |x: f32| -> f32 {
            static SEQ_VALUES: [f32; 3] = [200., 400., 800.];
            static mut SEQ_INDEX: usize = 0;
            static mut PREV_PULSE: f32 = 0.0;

            unsafe {
                if x > 0.5 && PREV_PULSE <= 0.5 {
                    SEQ_INDEX = (SEQ_INDEX + 1) % SEQ_VALUES.len();
                }

                PREV_PULSE = x;
                SEQ_VALUES[SEQ_INDEX]
            }
        });

        graph.connect(pulse_osc.output(), filter_a.input());
        graph.connect(pulse_osc.output(), filter_b.input());

        graph.connect(sequencer, filter_a.fmod());
        graph.connect(sequencer, filter_b.fmod());

        let filter_diff = graph.combine(filter_a.output(), filter_b.output(), |x, y| x - y);
        let limited_output = graph.transform(filter_diff, |x| x.tanh());
        let output = limited_output;

        let oscillator_freq_input = graph
            .insert_value_input(pulse_osc.frequency(), params.frequency.value())
            .ok_or("Failed to insert frequency input")?;

        let cutoff_freq_input_a = graph
            .insert_value_input(filter_a.cutoff(), params.cutoff_frequency_a.value())
            .ok_or("Failed to insert filter A cutoff input")?;

        let cutoff_freq_input_b = graph
            .insert_value_input(filter_b.cutoff(), params.cutoff_frequency_b.value())
            .ok_or("Failed to insert filter B cutoff input")?;

        let q_input_a = graph
            .insert_value_input(filter_a.resonance(), params.q_factor.value())
            .ok_or("Failed to insert filter A Q input")?;

        let q_input_b = graph
            .insert_value_input(filter_b.resonance(), params.q_factor.value())
            .ok_or("Failed to insert filter B Q input")?;

        Ok(Self {
            graph,
            oscillator_freq_input,
            cutoff_freq_input_a,
            cutoff_freq_input_b,
            q_input_a,
            q_input_b,
            output,
        })
    }

    fn update_params(&mut self, params: &TwinPeaksParams) {
        self.graph.set_value(
            self.oscillator_freq_input,
            params.frequency.smoothed.next(),
            441,
        );
        self.graph.set_value(
            self.cutoff_freq_input_a,
            params.cutoff_frequency_a.smoothed.next(),
            1323,
        );
        self.graph.set_value(
            self.cutoff_freq_input_b,
            params.cutoff_frequency_b.smoothed.next(),
            1323,
        );
        self.graph
            .set_value(self.q_input_a, params.q_factor.smoothed.next(), 441);
        self.graph
            .set_value(self.q_input_b, params.q_factor.smoothed.next(), 441);
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
    const NAME: &'static str = "Twin Peaks Demo";
    const VENDOR: &'static str = "Oscen";
    const URL: &'static str = "https://reed.nyc";
    const EMAIL: &'static str = "your.email@example.com";
    const VERSION: &'static str = "0.1.0";

    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[AudioIOLayout {
        main_input_channels: None,                // Synthesizer has no input
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
                                ui.heading("Oscillator");
                                ui.add_space(20.0);

                                ui.label("Trigger Frequency");
                                ui.add(nih_plug_egui::widgets::ParamSlider::for_param(
                                    &params.frequency,
                                    setter,
                                ));
                                ui.add_space(10.0);
                            });
                        });

                        ui.add_space(4.0);

                        ui.group(|ui| {
                            ui.vertical(|ui| {
                                ui.heading("Filter");
                                ui.add_space(20.0);

                                ui.label("Filter A Cutoff");
                                ui.add(nih_plug_egui::widgets::ParamSlider::for_param(
                                    &params.cutoff_frequency_a,
                                    setter,
                                ));
                                ui.add_space(10.0);

                                ui.label("Filter B Cutoff");
                                ui.add(nih_plug_egui::widgets::ParamSlider::for_param(
                                    &params.cutoff_frequency_b,
                                    setter,
                                ));
                                ui.add_space(10.0);

                                ui.label("Resonance (both filters)");
                                ui.add(nih_plug_egui::widgets::ParamSlider::for_param(
                                    &params.q_factor,
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
            for channel_samples in buffer.iter_samples() {
                // Update parameters
                audio_context.update_params(&self.params);

                // Process audio with Oscen
                audio_context.graph.process();

                if let Some(value) = audio_context.graph.get_value(&audio_context.output) {
                    // Write to all output channels (mono to stereo/multi-channel)
                    for sample in channel_samples {
                        *sample = value;
                    }
                }
            }
        }

        ProcessStatus::Normal
    }
}

impl ClapPlugin for TwinPeaks {
    const CLAP_ID: &'static str = "com.oscen.twin-peaks-nih";
    const CLAP_DESCRIPTION: Option<&'static str> = Some("Twin Peaks Filter Synthesizer");
    const CLAP_MANUAL_URL: Option<&'static str> = Some(Self::URL);
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] =
        &[ClapFeature::Instrument, ClapFeature::Synthesizer];
}

impl Vst3Plugin for TwinPeaks {
    const VST3_CLASS_ID: [u8; 16] = *b"TwinPeaksNIHPlug";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] =
        &[Vst3SubCategory::Instrument, Vst3SubCategory::Synth];
}

nih_export_clap!(TwinPeaks);
nih_export_vst3!(TwinPeaks);
