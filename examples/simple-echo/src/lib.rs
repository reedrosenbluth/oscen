use nih_plug::prelude::*;
use nih_plug_egui::{create_egui_editor, egui, EguiState};
use oscen::{
    filters::tpt::TptFilter, graph::ValueInput, Delay, Graph, StreamOutput, Value,
};
use parking_lot::RwLock;
use std::sync::Arc;

pub struct SimpleEcho {
    params: Arc<SimpleEchoParams>,
    audio_context: RwLock<Option<AudioContext>>,
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

pub struct ChannelContext {
    graph: Graph,
    delay_time_input: ValueInput,
    filter_cutoff_input: ValueInput,
    feedback_input: ValueInput,
    mix_input: ValueInput,
    output: StreamOutput,
    input_endpoint: ValueInput,
}

pub struct AudioContext {
    left: ChannelContext,
    right: ChannelContext,
}

fn build_channel_graph(
    sample_rate: f32,
    params: &SimpleEchoParams,
) -> Result<ChannelContext, &'static str> {
    let mut graph = Graph::new(sample_rate);

    let (input_signal, input_endpoint) = graph.add_audio_input();

    // Add nodes to graph
    let delay = graph.add_node(Delay::new(params.delay_time.value(), 0.0));
    let filter = graph.add_node(TptFilter::new(params.filter_cutoff.value(), 0.7));

    let feedback_node = graph.add_node(Value::new(params.feedback.value()));
    let mix_node = graph.add_node(Value::new(params.mix.value()));

    // Connect delay output to filter
    graph.connect(delay.output(), filter.input());

    // Create feedback loop with controllable amount
    let feedback_scaled = graph.combine(
        filter.output(),
        feedback_node.output(),
        |filtered, feedback| filtered * feedback, // Scale by feedback amount
    );

    // Mix input with feedback and send to delay (with limiter to prevent runaway)
    let delay_input = graph.combine(input_signal.output(), feedback_scaled, |input, feedback| {
        (input + feedback).tanh()
    });

    graph.connect(delay_input, delay.input());

    // Mix dry and wet signals with controllable mix
    let wet_signal = filter.output();
    let dry_signal = input_signal.output();

    // Create dry component (input * (1 - mix))
    let dry_mixed = graph.combine(dry_signal, mix_node.output(), |dry, mix| dry * (1.0 - mix));

    // Create wet component (wet * mix)
    let wet_mixed = graph.combine(wet_signal, mix_node.output(), |wet, mix| wet * mix);

    // Combine dry and wet
    let output = graph.combine(dry_mixed, wet_mixed, |dry, wet| dry + wet);

    // Set up parameter controls
    if graph
        .insert_value_input(delay.delay_time(), params.delay_time.value())
        .is_none()
    {
        return Err("Failed to insert delay time input");
    }

    if graph
        .insert_value_input(filter.cutoff(), params.filter_cutoff.value())
        .is_none()
    {
        return Err("Failed to insert filter cutoff input");
    }

    if graph
        .insert_value_input(feedback_node.input(), params.feedback.value())
        .is_none()
    {
        return Err("Failed to insert feedback input");
    }

    if graph
        .insert_value_input(mix_node.input(), params.mix.value())
        .is_none()
    {
        return Err("Failed to insert mix input");
    }

    Ok(ChannelContext {
        graph,
        delay_time_input: delay.delay_time(),
        filter_cutoff_input: filter.cutoff(),
        feedback_input: feedback_node.input(),
        mix_input: mix_node.input(),
        output,
        input_endpoint,
    })
}

impl AudioContext {
    fn new(sample_rate: f32, params: &SimpleEchoParams) -> Result<Self, &'static str> {
        let left = build_channel_graph(sample_rate, params)?;
        let right = build_channel_graph(sample_rate, params)?;

        Ok(Self { left, right })
    }

    fn update_params(&mut self, params: &SimpleEchoParams) {
        let delay_time = params.delay_time.smoothed.next();
        let filter_cutoff = params.filter_cutoff.smoothed.next();
        let feedback = params.feedback.smoothed.next();
        let mix = params.mix.smoothed.next();

        // Update left channel
        self.left
            .graph
            .set_value(self.left.delay_time_input, delay_time);
        self.left
            .graph
            .set_value(self.left.filter_cutoff_input, filter_cutoff);
        self.left
            .graph
            .set_value(self.left.feedback_input, feedback);
        self.left.graph.set_value(self.left.mix_input, mix);

        // Update right channel
        self.right
            .graph
            .set_value(self.right.delay_time_input, delay_time);
        self.right
            .graph
            .set_value(self.right.filter_cutoff_input, filter_cutoff);
        self.right
            .graph
            .set_value(self.right.feedback_input, feedback);
        self.right.graph.set_value(self.right.mix_input, mix);
    }
}

impl Default for SimpleEcho {
    fn default() -> Self {
        Self {
            params: Arc::new(SimpleEchoParams::default()),
            audio_context: RwLock::new(None),
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

                // Get input samples
                let inputs: Vec<f32> = channel_samples.iter_mut().map(|s| *s).collect();

                // Process based on channel count
                if inputs.len() >= 2 {
                    // Stereo processing
                    audio_context
                        .left
                        .graph
                        .set_value(audio_context.left.input_endpoint, inputs[0]);
                    audio_context
                        .right
                        .graph
                        .set_value(audio_context.right.input_endpoint, inputs[1]);

                    let _ = audio_context.left.graph.process();
                    let _ = audio_context.right.graph.process();

                    let output_left = audio_context
                        .left
                        .graph
                        .get_value(&audio_context.left.output)
                        .unwrap_or(0.0);
                    let output_right = audio_context
                        .right
                        .graph
                        .get_value(&audio_context.right.output)
                        .unwrap_or(0.0);

                    for (i, sample) in channel_samples.into_iter().enumerate() {
                        *sample = if i == 0 { output_left } else { output_right };
                    }
                } else {
                    // Mono processing - just use left channel
                    audio_context
                        .left
                        .graph
                        .set_value(audio_context.left.input_endpoint, inputs[0]);
                    let _ = audio_context.left.graph.process();

                    let output_mono = audio_context
                        .left
                        .graph
                        .get_value(&audio_context.left.output)
                        .unwrap_or(0.0);

                    //TODO: make stereo
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
