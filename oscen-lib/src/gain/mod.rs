use crate::graph::{
    InputEndpoint, NodeKey, ProcessingContext, ProcessingNode, SignalProcessor,
    ValueKey,
};
use crate::Node;

/// Gain node using CMajor-style direct field access.
///
/// The #[derive(Node)] macro generates:
/// - GainEndpoints: typed endpoint handles
///
/// Inputs and outputs are PUBLIC fields in this struct.
/// For compile-time graphs, connections compile to direct field assignments.
/// For runtime graphs, accessor methods route data dynamically.
#[derive(Debug, Node)]
pub struct Gain {
    #[input(stream)]
    pub input: f32,

    #[input(value)]
    pub gain: f32,

    #[output(stream)]
    pub output: f32,
}

impl Gain {
    pub fn new(initial_gain: f32) -> Self {
        Self {
            input: 0.0,
            gain: initial_gain,
            output: 0.0,
        }
    }

    /// User-defined processing logic - this is all the user writes!
    pub fn process_dsp(&mut self, _sample_rate: f32) {
        self.output = self.input * self.gain;
    }
}

impl Default for Gain {
    fn default() -> Self {
        Self::new(1.0)
    }
}

impl SignalProcessor for Gain {
    /// Auto-populated implementation (TODO: auto-generate via macro)
    fn process<'a>(&mut self, sample_rate: f32, context: &mut ProcessingContext<'a>) {
        // Populate input fields from context
        self.input = context.stream(0);

        // Populate value inputs from context
        if let Some(value_ref) = context.value(0) {
            if let Some(scalar) = value_ref.as_scalar() {
                self.gain = scalar;
            }
        }

        // Call user's process method
        self.process_dsp(sample_rate);

        // Output is now in self.output - runtime graph reads it via get_stream_output()
    }

    // Accessor methods for runtime graph routing
    fn get_stream_output(&self, index: usize) -> Option<f32> {
        match index {
            0 => Some(self.output),
            _ => None,
        }
    }

    fn set_stream_input(&mut self, index: usize, value: f32) {
        match index {
            0 => self.input = value,
            _ => {}
        }
    }
}
