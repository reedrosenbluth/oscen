use nih_plug::prelude::*;
use nih_plug_egui::{create_egui_editor, egui, EguiState};
use oscen::delay::Delay;
use oscen::filters::tpt::TptFilter;
use oscen::SignalProcessor;
use parking_lot::RwLock;
use std::sync::Arc;

/// A single channel of the echo effect
pub struct EchoChannel {
    delay: Delay,
    filter: TptFilter,
    sample_rate: f32,
}

impl EchoChannel {
    fn new(sample_rate: f32) -> Self {
        let mut delay = Delay::new(11025.0, 0.0);  // 0.25s at 44.1kHz, no internal feedback
        delay.init(sample_rate);

        let mut filter = TptFilter::new(4000.0, 0.7);
        filter.init(sample_rate);

        Self {
            delay,
            filter,
            sample_rate,
        }
    }

    fn process(&mut self, input: f32, delay_time: f32, filter_cutoff: f32, feedback: f32, mix: f32) -> f32 {
        // Update delay time (convert seconds to samples)
        let delay_samples = delay_time * self.sample_rate;

        // Get feedback from previous filter output
        let feedback_signal = self.filter.output * feedback;

        // Feed input + feedback into delay (with soft clipping to prevent runaway)
        self.delay.input = (input + feedback_signal).tanh();

        // Update filter cutoff
        self.filter.cutoff = filter_cutoff;

        // Process delay -> filter chain
        self.delay.process();
        self.filter.input = self.delay.output;
        self.filter.process();

        // Mix dry and wet
        let wet = self.filter.output;
        input * (1.0 - mix) + wet * mix
    }
}

pub struct SimpleEcho {
    params: Arc<SimpleEchoParams>,
    left: RwLock<Option<EchoChannel>>,
    right: RwLock<Option<EchoChannel>>,
}

#[derive(Params)]
pub struct SimpleEchoParams {
    #[persist = "editor-state"]
    editor_state: Arc<EguiState>,

    #[id = "delay_time"]
    pub delay_time: FloatParam,

    #[id = "feedback"]
    pub feedback: FloatParam,

    #[id = "filter_cutoff"]
    pub filter_cutoff: FloatParam,

    #[id = "mix"]
    pub mix: FloatParam,
}

impl Default for SimpleEchoParams {
    fn default() -> Self {
        Self {
            editor_state: EguiState::from_size(250, 300),

            delay_time: FloatParam::new(
                "Delay Time",
                0.25,
                FloatRange::Linear {
                    min: 0.01,
                    max: 1.0,
                },
            )
            .with_smoother(SmoothingStyle::Linear(50.0))
            .with_unit(" s")
            .with_value_to_string(formatters::v2s_f32_rounded(2)),

            feedback: FloatParam::new(
                "Feedback",
                0.5,
                FloatRange::Linear {
                    min: 0.0,
                    max: 0.95,
                },
            )
            .with_smoother(SmoothingStyle::Linear(50.0))
            .with_value_to_string(formatters::v2s_f32_rounded(2)),

            filter_cutoff: FloatParam::new(
                "Filter Cutoff",
                4000.0,
                FloatRange::Skewed {
                    min: 100.0,
                    max: 10000.0,
                    factor: FloatRange::skew_factor(-1.5),
                },
            )
            .with_smoother(SmoothingStyle::Linear(50.0))
            .with_unit(" Hz")
            .with_value_to_string(formatters::v2s_f32_rounded(0)),

            mix: FloatParam::new("Mix", 0.5, FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_smoother(SmoothingStyle::Linear(50.0))
                .with_value_to_string(formatters::v2s_f32_rounded(2)),
        }
    }
}

impl Default for SimpleEcho {
    fn default() -> Self {
        Self {
            params: Arc::new(SimpleEchoParams::default()),
            left: RwLock::new(None),
            right: RwLock::new(None),
        }
    }
}

impl Plugin for SimpleEcho {
    const NAME: &'static str = "Simple Echo";
    const VENDOR: &'static str = "Oscen";
    const URL: &'static str = "https://reed.nyc";
    const EMAIL: &'static str = "your.email@example.com";
    const VERSION: &'static str = "0.1.0";

    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[
        AudioIOLayout {
            main_input_channels: NonZeroU32::new(2),  // Stereo input
            main_output_channels: NonZeroU32::new(2), // Stereo output
            aux_input_ports: &[],
            aux_output_ports: &[],
            names: PortNames::const_default(),
        },
        AudioIOLayout {
            main_input_channels: NonZeroU32::new(1),  // Mono input
            main_output_channels: NonZeroU32::new(1), // Mono output
            aux_input_ports: &[],
            aux_output_ports: &[],
            names: PortNames::const_default(),
        },
    ];

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
                    ui.vertical_centered(|ui| {
                        ui.heading("Simple Echo");
                        ui.add_space(20.0);

                        ui.group(|ui| {
                            ui.vertical(|ui| {
                                ui.label("Delay Time");
                                ui.add(nih_plug_egui::widgets::ParamSlider::for_param(
                                    &params.delay_time,
                                    setter,
                                ));
                                ui.add_space(10.0);

                                ui.label("Feedback");
                                ui.add(nih_plug_egui::widgets::ParamSlider::for_param(
                                    &params.feedback,
                                    setter,
                                ));
                                ui.add_space(10.0);

                                ui.label("Filter Cutoff");
                                ui.add(nih_plug_egui::widgets::ParamSlider::for_param(
                                    &params.filter_cutoff,
                                    setter,
                                ));
                                ui.add_space(10.0);

                                ui.label("Mix");
                                ui.add(nih_plug_egui::widgets::ParamSlider::for_param(
                                    &params.mix,
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

        *self.left.write() = Some(EchoChannel::new(sample_rate));
        *self.right.write() = Some(EchoChannel::new(sample_rate));

        true
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        _aux: &mut AuxiliaryBuffers,
        _context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        let mut left_guard = self.left.write();
        let mut right_guard = self.right.write();

        if let (Some(left), Some(right)) = (left_guard.as_mut(), right_guard.as_mut()) {
            for mut channel_samples in buffer.iter_samples() {
                // Get smoothed parameter values
                let delay_time = self.params.delay_time.smoothed.next();
                let filter_cutoff = self.params.filter_cutoff.smoothed.next();
                let feedback = self.params.feedback.smoothed.next();
                let mix = self.params.mix.smoothed.next();

                // Get input samples
                let inputs: Vec<f32> = channel_samples.iter_mut().map(|s| *s).collect();

                // Process based on channel count
                if inputs.len() >= 2 {
                    // Stereo processing
                    let output_left = left.process(inputs[0], delay_time, filter_cutoff, feedback, mix);
                    let output_right = right.process(inputs[1], delay_time, filter_cutoff, feedback, mix);

                    for (i, sample) in channel_samples.into_iter().enumerate() {
                        *sample = if i == 0 { output_left } else { output_right };
                    }
                } else {
                    // Mono processing - just use left channel
                    let output_mono = left.process(inputs[0], delay_time, filter_cutoff, feedback, mix);

                    for sample in channel_samples {
                        *sample = output_mono;
                    }
                }
            }
        }

        ProcessStatus::Normal
    }
}

impl ClapPlugin for SimpleEcho {
    const CLAP_ID: &'static str = "com.oscen.simple-echo";
    const CLAP_DESCRIPTION: Option<&'static str> = Some("Simple Echo with Feedback");
    const CLAP_MANUAL_URL: Option<&'static str> = Some(Self::URL);
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] = &[ClapFeature::AudioEffect, ClapFeature::Delay];
}

impl Vst3Plugin for SimpleEcho {
    const VST3_CLASS_ID: [u8; 16] = *b"SimpleEchoOscen ";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] =
        &[Vst3SubCategory::Fx, Vst3SubCategory::Delay];
}

nih_export_clap!(SimpleEcho);
nih_export_vst3!(SimpleEcho);
