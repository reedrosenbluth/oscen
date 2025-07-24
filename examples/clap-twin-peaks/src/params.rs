use crate::{TwinPeaksPluginAudioProcessor, TwinPeaksPluginMainThread};
use clack_extensions::params::*;
use clack_extensions::state::PluginStateImpl;
use clack_plugin::events::spaces::CoreEventSpace;
use clack_plugin::prelude::*;
use clack_plugin::stream::{InputStream, OutputStream};
use std::ffi::CStr;
use std::fmt::Write as _;
use std::io::{Read, Write as _};
use std::sync::atomic::{AtomicU32, Ordering};

pub const PARAM_FREQUENCY_ID: ClapId = ClapId::new(1);
pub const PARAM_CUTOFF_A_ID: ClapId = ClapId::new(2);
pub const PARAM_CUTOFF_B_ID: ClapId = ClapId::new(3);
pub const PARAM_Q_FACTOR_ID: ClapId = ClapId::new(4);

const DEFAULT_FREQUENCY: f32 = 3.0;
const DEFAULT_CUTOFF_A: f32 = 1000.0;
const DEFAULT_CUTOFF_B: f32 = 1900.0;
const DEFAULT_Q_FACTOR: f32 = 0.54;

#[derive(Clone, Copy, Debug)]
pub struct SynthParams {
    pub frequency: f32,
    pub cutoff_frequency_a: f32,
    pub cutoff_frequency_b: f32,
    pub q_factor: f32,
}

impl Default for SynthParams {
    fn default() -> Self {
        Self {
            frequency: DEFAULT_FREQUENCY,
            cutoff_frequency_a: DEFAULT_CUTOFF_A,
            cutoff_frequency_b: DEFAULT_CUTOFF_B,
            q_factor: DEFAULT_Q_FACTOR,
        }
    }
}

pub struct TwinPeaksParams {
    frequency: AtomicF32,
    cutoff_frequency_a: AtomicF32,
    cutoff_frequency_b: AtomicF32,
    q_factor: AtomicF32,
}

impl TwinPeaksParams {
    pub fn new() -> Self {
        Self {
            frequency: AtomicF32::new(DEFAULT_FREQUENCY),
            cutoff_frequency_a: AtomicF32::new(DEFAULT_CUTOFF_A),
            cutoff_frequency_b: AtomicF32::new(DEFAULT_CUTOFF_B),
            q_factor: AtomicF32::new(DEFAULT_Q_FACTOR),
        }
    }

    pub fn get_params(&self) -> SynthParams {
        SynthParams {
            frequency: self.frequency.load(Ordering::SeqCst),
            cutoff_frequency_a: self.cutoff_frequency_a.load(Ordering::SeqCst),
            cutoff_frequency_b: self.cutoff_frequency_b.load(Ordering::SeqCst),
            q_factor: self.q_factor.load(Ordering::SeqCst),
        }
    }

    pub fn set_frequency(&self, value: f32) {
        let value = value.clamp(0.1, 10.0);
        self.frequency.store(value, Ordering::SeqCst);
    }

    pub fn set_cutoff_a(&self, value: f32) {
        let value = value.clamp(20.0, 14500.0);
        self.cutoff_frequency_a.store(value, Ordering::SeqCst);
    }

    pub fn set_cutoff_b(&self, value: f32) {
        let value = value.clamp(20.0, 14500.0);
        self.cutoff_frequency_b.store(value, Ordering::SeqCst);
    }

    pub fn set_q_factor(&self, value: f32) {
        let value = value.clamp(0.4, 0.99);
        self.q_factor.store(value, Ordering::SeqCst);
    }

    pub fn handle_event(&self, event: &UnknownEvent) {
        if let Some(CoreEventSpace::ParamValue(event)) = event.as_core_event() {
            if event.param_id() == PARAM_FREQUENCY_ID {
                self.set_frequency(event.value() as f32);
            } else if event.param_id() == PARAM_CUTOFF_A_ID {
                self.set_cutoff_a(event.value() as f32);
            } else if event.param_id() == PARAM_CUTOFF_B_ID {
                self.set_cutoff_b(event.value() as f32);
            } else if event.param_id() == PARAM_Q_FACTOR_ID {
                self.set_q_factor(event.value() as f32);
            }
        }
    }
}

impl PluginStateImpl for TwinPeaksPluginMainThread<'_> {
    fn save(&mut self, output: &mut OutputStream) -> Result<(), PluginError> {
        let params = self.shared.params.get_params();

        output.write_all(&params.frequency.to_le_bytes())?;
        output.write_all(&params.cutoff_frequency_a.to_le_bytes())?;
        output.write_all(&params.cutoff_frequency_b.to_le_bytes())?;
        output.write_all(&params.q_factor.to_le_bytes())?;
        Ok(())
    }

    fn load(&mut self, input: &mut InputStream) -> Result<(), PluginError> {
        let mut buf = [0; 4];

        input.read_exact(&mut buf)?;
        let frequency = f32::from_le_bytes(buf);
        self.shared.params.set_frequency(frequency);

        input.read_exact(&mut buf)?;
        let cutoff_a = f32::from_le_bytes(buf);
        self.shared.params.set_cutoff_a(cutoff_a);

        input.read_exact(&mut buf)?;
        let cutoff_b = f32::from_le_bytes(buf);
        self.shared.params.set_cutoff_b(cutoff_b);

        input.read_exact(&mut buf)?;
        let q_factor = f32::from_le_bytes(buf);
        self.shared.params.set_q_factor(q_factor);

        Ok(())
    }
}

impl PluginMainThreadParams for TwinPeaksPluginMainThread<'_> {
    fn count(&mut self) -> u32 {
        4
    }

    fn get_info(&mut self, param_index: u32, info: &mut ParamInfoWriter) {
        match param_index {
            0 => info.set(&ParamInfo {
                id: PARAM_FREQUENCY_ID,
                flags: ParamInfoFlags::IS_AUTOMATABLE,
                cookie: Default::default(),
                name: b"Frequency",
                module: b"Oscillator",
                min_value: 0.1,
                max_value: 10.0,
                default_value: DEFAULT_FREQUENCY as f64,
            }),
            1 => info.set(&ParamInfo {
                id: PARAM_CUTOFF_A_ID,
                flags: ParamInfoFlags::IS_AUTOMATABLE,
                cookie: Default::default(),
                name: b"Cutoff A",
                module: b"Filter",
                min_value: 20.0,
                max_value: 14500.0,
                default_value: DEFAULT_CUTOFF_A as f64,
            }),
            2 => info.set(&ParamInfo {
                id: PARAM_CUTOFF_B_ID,
                flags: ParamInfoFlags::IS_AUTOMATABLE,
                cookie: Default::default(),
                name: b"Cutoff B",
                module: b"Filter",
                min_value: 20.0,
                max_value: 14500.0,
                default_value: DEFAULT_CUTOFF_B as f64,
            }),
            3 => info.set(&ParamInfo {
                id: PARAM_Q_FACTOR_ID,
                flags: ParamInfoFlags::IS_AUTOMATABLE,
                cookie: Default::default(),
                name: b"Q Factor",
                module: b"Filter",
                min_value: 0.4,
                max_value: 0.99,
                default_value: DEFAULT_Q_FACTOR as f64,
            }),
            _ => {}
        }
    }

    fn get_value(&mut self, param_id: ClapId) -> Option<f64> {
        match param_id {
            PARAM_FREQUENCY_ID => Some(self.shared.params.frequency.load(Ordering::SeqCst) as f64),
            PARAM_CUTOFF_A_ID => {
                Some(self.shared.params.cutoff_frequency_a.load(Ordering::SeqCst) as f64)
            }
            PARAM_CUTOFF_B_ID => {
                Some(self.shared.params.cutoff_frequency_b.load(Ordering::SeqCst) as f64)
            }
            PARAM_Q_FACTOR_ID => Some(self.shared.params.q_factor.load(Ordering::SeqCst) as f64),
            _ => None,
        }
    }

    fn value_to_text(
        &mut self,
        param_id: ClapId,
        value: f64,
        writer: &mut ParamDisplayWriter,
    ) -> std::fmt::Result {
        match param_id {
            PARAM_FREQUENCY_ID => write!(writer, "{:.1} Hz", value),
            PARAM_CUTOFF_A_ID | PARAM_CUTOFF_B_ID => write!(writer, "{:.0} Hz", value),
            PARAM_Q_FACTOR_ID => write!(writer, "{:.3}", value),
            _ => Err(std::fmt::Error),
        }
    }

    fn text_to_value(&mut self, param_id: ClapId, text: &CStr) -> Option<f64> {
        let text = text.to_str().ok()?;
        let text = text.trim();

        match param_id {
            PARAM_FREQUENCY_ID => {
                let text = text.strip_suffix("Hz").unwrap_or(text).trim();
                text.parse().ok()
            }
            PARAM_CUTOFF_A_ID | PARAM_CUTOFF_B_ID => {
                let text = text.strip_suffix("Hz").unwrap_or(text).trim();
                text.parse().ok()
            }
            PARAM_Q_FACTOR_ID => text.parse().ok(),
            _ => None,
        }
    }

    fn flush(
        &mut self,
        input_parameter_changes: &InputEvents,
        _output_parameter_changes: &mut OutputEvents,
    ) {
        for event in input_parameter_changes {
            self.shared.params.handle_event(event)
        }
    }
}

impl PluginAudioProcessorParams for TwinPeaksPluginAudioProcessor<'_> {
    fn flush(
        &mut self,
        input_parameter_changes: &InputEvents,
        _output_parameter_changes: &mut OutputEvents,
    ) {
        for event in input_parameter_changes {
            self.shared.params.handle_event(event)
        }
    }
}

struct AtomicF32(AtomicU32);

impl AtomicF32 {
    #[inline]
    fn new(value: f32) -> Self {
        Self(AtomicU32::new(f32_to_u32_bytes(value)))
    }

    #[inline]
    fn store(&self, value: f32, order: Ordering) {
        self.0.store(f32_to_u32_bytes(value), order)
    }

    #[inline]
    fn load(&self, order: Ordering) -> f32 {
        f32_from_u32_bytes(self.0.load(order))
    }
}

#[inline]
fn f32_to_u32_bytes(value: f32) -> u32 {
    u32::from_ne_bytes(value.to_ne_bytes())
}

#[inline]
fn f32_from_u32_bytes(bytes: u32) -> f32 {
    f32::from_ne_bytes(bytes.to_ne_bytes())
}
