use crate::graph::{
    InputEndpoint, NodeKey, ProcessingContext, ProcessingNode, SignalProcessor, ValueKey,
};
use crate::Node;

/// Gain node using struct-of-arrays IO pattern.
///
/// The #[derive(Node)] macro generates:
/// - GainIO struct: holds stream I/O (input, output fields)
/// - GainEndpoints: typed endpoint handles
///
/// State (this struct): holds persistent data and value inputs
/// IO (generated GainIO): holds per-sample stream data
#[derive(Debug, Node)]
pub struct Gain {
    #[input(stream)]
    input: f32,

    #[input(value)]
    gain: f32,

    #[output(stream)]
    output: f32,

    /// Persistent IO struct - reused every call for zero overhead.
    /// Public so compile-time graphs can wire connections directly.
    pub io: GainIO,
}

impl Gain {
    pub fn new(initial_gain: f32) -> Self {
        Self {
            input: 0.0,      // Placeholder for endpoint descriptor
            gain: initial_gain,  // Initial value for gain parameter
            output: 0.0,     // Placeholder for endpoint descriptor
            io: GainIO {
                input: 0.0,
                output: 0.0,
            },
        }
    }

    /// Internal processing logic - works for both runtime and compile-time graphs.
    ///
    /// Assumes self.io is already populated (either from context or direct wiring).
    /// This is the ONLY place where processing logic is implemented.
    #[inline]
    pub fn process_internal(&mut self) -> f32 {
        // Process using self.io (already populated)
        self.io.output = self.io.input * self.gain;
        self.io.output
    }
}

impl Default for Gain {
    fn default() -> Self {
        Self::new(1.0)
    }
}

impl SignalProcessor for Gain {
    /// Runtime graph entry point - populates IO from context, then processes.
    ///
    /// For compile-time graphs, wire self.io directly and call process_internal() instead.
    fn process<'a>(&mut self, _sample_rate: f32, context: &mut ProcessingContext<'a>) -> f32 {
        // Populate self.io from context (runtime graphs only)
        self.io.input = self.get_input(context);

        // Get value inputs from context
        let gain_mod = self.get_gain(context);
        if gain_mod != 0.0 {
            self.gain = gain_mod; // Allow runtime modulation of gain
        }

        // Call shared processing logic
        self.process_internal()
    }
}
