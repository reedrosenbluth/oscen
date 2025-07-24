use crate::params::{SynthParams, TwinPeaksParams};
use clack_extensions::state::PluginState;
use clack_extensions::{audio_ports::*, params::*};
use clack_plugin::prelude::*;
use oscen::{Graph, LP18Filter, Oscillator, OutputEndpoint, ValueKey};

mod params;

pub struct TwinPeaksPlugin;

impl Plugin for TwinPeaksPlugin {
    type AudioProcessor<'a> = TwinPeaksPluginAudioProcessor<'a>;
    type Shared<'a> = TwinPeaksPluginShared;
    type MainThread<'a> = TwinPeaksPluginMainThread<'a>;

    fn declare_extensions(
        builder: &mut PluginExtensions<Self>,
        _shared: Option<&TwinPeaksPluginShared>,
    ) {
        builder
            .register::<PluginAudioPorts>()
            .register::<PluginParams>()
            .register::<PluginState>();
    }
}

impl DefaultPluginFactory for TwinPeaksPlugin {
    fn get_descriptor() -> PluginDescriptor {
        use clack_plugin::plugin::features::*;

        PluginDescriptor::new("org.rust-audio.clack.twin-peaks", "Twin Peaks Synth")
            .with_features([SYNTHESIZER, INSTRUMENT, MONO])
    }

    fn new_shared(_host: HostSharedHandle) -> Result<Self::Shared<'_>, PluginError> {
        Ok(TwinPeaksPluginShared {
            params: TwinPeaksParams::new(),
        })
    }

    fn new_main_thread<'a>(
        _host: HostMainThreadHandle<'a>,
        shared: &'a Self::Shared<'a>,
    ) -> Result<Self::MainThread<'a>, PluginError> {
        Ok(Self::MainThread { shared })
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
    fn new(sample_rate: f32, default_params: SynthParams) -> Result<Self, PluginError> {
        let mut graph = Graph::new(sample_rate);

        let pulse_osc = graph.add_node(Oscillator::new(default_params.frequency, 1.0, |p| {
            if p < 0.001 {
                1.0
            } else {
                0.0
            }
        }));

        let filter_a = graph.add_node(LP18Filter::new(
            default_params.cutoff_frequency_a,
            default_params.q_factor,
        ));
        let filter_b = graph.add_node(LP18Filter::new(
            default_params.cutoff_frequency_b,
            default_params.q_factor,
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
            .insert_value_input(pulse_osc.frequency(), default_params.frequency)
            .ok_or(PluginError::Message("Failed to insert frequency input"))?;

        let cutoff_freq_input_a = graph
            .insert_value_input(filter_a.cutoff(), default_params.cutoff_frequency_a)
            .ok_or(PluginError::Message(
                "Failed to insert filter A cutoff input",
            ))?;

        let cutoff_freq_input_b = graph
            .insert_value_input(filter_b.cutoff(), default_params.cutoff_frequency_b)
            .ok_or(PluginError::Message(
                "Failed to insert filter B cutoff input",
            ))?;

        let q_input_a = graph
            .insert_value_input(filter_a.resonance(), default_params.q_factor)
            .ok_or(PluginError::Message("Failed to insert filter A Q input"))?;

        let q_input_b = graph
            .insert_value_input(filter_b.resonance(), default_params.q_factor)
            .ok_or(PluginError::Message("Failed to insert filter B Q input"))?;

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

    fn update_params(&mut self, params: SynthParams) {
        self.graph
            .set_value(self.oscillator_freq_input, params.frequency, 441);
        self.graph
            .set_value(self.cutoff_freq_input_a, params.cutoff_frequency_a, 1323);
        self.graph
            .set_value(self.cutoff_freq_input_b, params.cutoff_frequency_b, 1323);
        self.graph.set_value(self.q_input_a, params.q_factor, 441);
        self.graph.set_value(self.q_input_b, params.q_factor, 441);
    }
}

pub struct TwinPeaksPluginAudioProcessor<'a> {
    shared: &'a TwinPeaksPluginShared,
    audio_context: Option<AudioContext>,
}

impl<'a> PluginAudioProcessor<'a, TwinPeaksPluginShared, TwinPeaksPluginMainThread<'a>>
    for TwinPeaksPluginAudioProcessor<'a>
{
    fn activate(
        _host: HostAudioProcessorHandle<'a>,
        _main_thread: &mut TwinPeaksPluginMainThread,
        shared: &'a TwinPeaksPluginShared,
        audio_config: PluginAudioConfiguration,
    ) -> Result<Self, PluginError> {
        let sample_rate = audio_config.sample_rate as f32;
        let default_params = shared.params.get_params();
        let audio_context = AudioContext::new(sample_rate, default_params)?;

        Ok(Self {
            shared,
            audio_context: Some(audio_context),
        })
    }

    fn process(
        &mut self,
        _process: Process,
        mut audio: Audio,
        events: Events,
    ) -> Result<ProcessStatus, PluginError> {
        let mut port_pair = audio
            .port_pair(0)
            .ok_or(PluginError::Message("No input/output ports found"))?;

        let mut output_channels = port_pair
            .channels()?
            .into_f32()
            .ok_or(PluginError::Message("Expected f32 input/output"))?;

        let mut channel_buffers = [None];

        // Extract output buffers (following clap-gain pattern)
        for (pair, buf) in output_channels.iter_mut().zip(&mut channel_buffers) {
            *buf = match pair {
                ChannelPair::InputOnly(_) => None,
                ChannelPair::OutputOnly(o) => Some(o),
                ChannelPair::InPlace(b) => Some(b),
                ChannelPair::InputOutput(_, o) => Some(o), // For synth, ignore input
            }
        }

        if let Some(audio_context) = &mut self.audio_context {
            // Process audio in batches for sample-accurate automation (following clap-gain pattern)
            for event_batch in events.input.batch() {
                // Process all parameter events in this batch
                for event in event_batch.events() {
                    self.shared.params.handle_event(event);
                }

                // Update parameters after processing events
                let params = self.shared.params.get_params();
                audio_context.update_params(params);

                // Process audio for this batch (mono output)
                for buf in channel_buffers.iter_mut().flatten() {
                    for sample in buf.iter_mut() {
                        audio_context.graph.process();
                        if let Some(value) = audio_context.graph.get_value(&audio_context.output) {
                            *sample = value;
                        }
                    }
                }
            }
        }

        Ok(ProcessStatus::ContinueIfNotQuiet)
    }
}

impl PluginAudioPortsImpl for TwinPeaksPluginMainThread<'_> {
    fn count(&mut self, is_input: bool) -> u32 {
        if is_input {
            0 // Synthesizer has no input ports
        } else {
            1 // One mono output port
        }
    }

    fn get(&mut self, index: u32, is_input: bool, writer: &mut AudioPortInfoWriter) {
        if !is_input && index == 0 {
            writer.set(&AudioPortInfo {
                id: ClapId::new(1),
                name: b"main",
                channel_count: 1,
                flags: AudioPortFlags::IS_MAIN,
                port_type: Some(AudioPortType::MONO),
                in_place_pair: None,
            });
        }
    }
}

pub struct TwinPeaksPluginShared {
    params: TwinPeaksParams,
}

impl PluginShared<'_> for TwinPeaksPluginShared {}

pub struct TwinPeaksPluginMainThread<'a> {
    shared: &'a TwinPeaksPluginShared,
}

impl<'a> PluginMainThread<'a, TwinPeaksPluginShared> for TwinPeaksPluginMainThread<'a> {}

clack_export_entry!(SinglePluginEntry<TwinPeaksPlugin>);
