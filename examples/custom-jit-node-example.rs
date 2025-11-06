// Example: Creating a Custom Node with JIT Support
//
// This demonstrates how users of Oscen can create custom nodes that
// benefit from JIT compilation.

use oscen::{Node, ProcessingContext, ProcessingNode, SignalProcessor};
use oscen::jit::{CodegenContext, CodegenError, JITCodegen};
use std::collections::HashMap;

/// A simple one-pole lowpass filter
///
/// This demonstrates:
/// - Custom node implementation
/// - JIT codegen support
/// - State management
/// - Parameter handling
#[derive(Debug, Node)]
pub struct OnePoleFilter {
    /// Audio input
    #[input(stream)]
    input: f32,

    /// Cutoff coefficient (0.0 to 1.0)
    /// Higher values = more high frequency content
    #[input(value)]
    cutoff: f32,

    /// Filtered output
    #[output(stream)]
    output: f32,

    /// Internal filter state
    state: f32,
}

impl OnePoleFilter {
    pub fn new(cutoff: f32) -> Self {
        Self {
            input: 0.0,
            cutoff,
            output: 0.0,
            state: 0.0,
        }
    }
}

/// Standard interpreted implementation
impl SignalProcessor for OnePoleFilter {
    fn process(&mut self, _sample_rate: f32, context: &mut ProcessingContext) -> f32 {
        // Create IO struct (struct-of-arrays pattern)
        let mut io = OnePoleFilterIO {
            input: self.get_input(context),
            output: 0.0,
        };

        // Get parameter
        let cutoff = self.get_cutoff(context);

        // Process: state += (input - state) * cutoff
        // This is a simple exponential moving average
        self.state += (io.input - self.state) * cutoff;
        io.output = self.state;

        io.output
    }

    /// Enable JIT compilation for this node
    fn as_jit_codegen(&self) -> Option<&dyn JITCodegen> {
        Some(self)
    }
}

/// JIT codegen implementation
impl JITCodegen for OnePoleFilter {
    fn emit_ir(&self, ctx: &mut CodegenContext) -> Result<(), CodegenError> {
        // Memory layout:
        // - State: [state: f32] at offset 0
        // - IO: [input: f32, output: f32] at offsets 0, 4
        // - Params: [cutoff: f32] at offset 0

        // 1. Load input from IO buffer
        let input = ctx.load_io(0);

        // 2. Load cutoff parameter
        let cutoff = ctx.load_param(0);

        // 3. Load state
        let state = ctx.load_state(0);

        // 4. Compute: input - state
        let diff = ctx.builder.ins().fsub(input, state);

        // 5. Compute: diff * cutoff
        let delta = ctx.builder.ins().fmul(diff, cutoff);

        // 6. Compute: new_state = state + delta
        let new_state = ctx.builder.ins().fadd(state, delta);

        // 7. Store updated state
        ctx.store_state(new_state, 0);

        // 8. Store output (output = new_state)
        ctx.store_io(new_state, 4);  // Output at offset 4

        Ok(())
    }

    fn jit_state_size(&self) -> usize {
        4  // One f32 for state
    }

    fn jit_io_size(&self) -> usize {
        8  // input: f32 (4 bytes) + output: f32 (4 bytes)
    }

    fn jit_param_count(&self) -> usize {
        1  // cutoff parameter
    }

    fn jit_io_field_offsets(&self) -> HashMap<usize, usize> {
        let mut offsets = HashMap::new();
        offsets.insert(0, 0);  // input at offset 0
        offsets.insert(1, 4);  // output at offset 4
        offsets
    }
}

/// A more complex example: Resonant filter with multiple parameters
#[derive(Debug, Node)]
pub struct ResonantFilter {
    #[input(stream)]
    input: f32,

    #[input(value)]
    cutoff: f32,

    #[input(value)]
    resonance: f32,

    #[output(stream)]
    output: f32,

    // State variables for 2-pole filter
    z1: f32,
    z2: f32,
}

impl ResonantFilter {
    pub fn new(cutoff: f32, resonance: f32) -> Self {
        Self {
            input: 0.0,
            cutoff,
            resonance,
            output: 0.0,
            z1: 0.0,
            z2: 0.0,
        }
    }
}

impl SignalProcessor for ResonantFilter {
    fn process(&mut self, _sample_rate: f32, context: &mut ProcessingContext) -> f32 {
        let mut io = ResonantFilterIO {
            input: self.get_input(context),
            output: 0.0,
        };

        let cutoff = self.get_cutoff(context);
        let resonance = self.get_resonance(context);

        // Simplified 2-pole resonant filter
        // Real implementation would be more sophisticated
        let feedback = self.z2 * resonance;
        let hp = io.input - self.z1 - feedback;
        let bp = hp * cutoff + self.z1;
        let lp = bp * cutoff + self.z2;

        self.z1 = bp;
        self.z2 = lp;

        io.output = lp;
        io.output
    }

    fn as_jit_codegen(&self) -> Option<&dyn JITCodegen> {
        Some(self)
    }
}

impl JITCodegen for ResonantFilter {
    fn emit_ir(&self, ctx: &mut CodegenContext) -> Result<(), CodegenError> {
        // Memory layout:
        // State: [z1: f32, z2: f32] at offsets 0, 4
        // IO: [input: f32, output: f32] at offsets 0, 4
        // Params: [cutoff: f32, resonance: f32] at offsets 0, 4

        // Load everything
        let input = ctx.load_io(0);
        let cutoff = ctx.load_param(0);
        let resonance = ctx.load_param(1);
        let z1 = ctx.load_state(0);
        let z2 = ctx.load_state(4);

        // Compute: feedback = z2 * resonance
        let feedback = ctx.builder.ins().fmul(z2, resonance);

        // Compute: hp = input - z1 - feedback
        let tmp1 = ctx.builder.ins().fsub(input, z1);
        let hp = ctx.builder.ins().fsub(tmp1, feedback);

        // Compute: bp = hp * cutoff + z1
        let hp_scaled = ctx.builder.ins().fmul(hp, cutoff);
        let bp = ctx.builder.ins().fadd(hp_scaled, z1);

        // Compute: lp = bp * cutoff + z2
        let bp_scaled = ctx.builder.ins().fmul(bp, cutoff);
        let lp = ctx.builder.ins().fadd(bp_scaled, z2);

        // Update state
        ctx.store_state(bp, 0);  // z1 = bp
        ctx.store_state(lp, 4);  // z2 = lp

        // Store output
        ctx.store_io(lp, 4);

        Ok(())
    }

    fn jit_state_size(&self) -> usize {
        8  // z1 + z2
    }

    fn jit_io_size(&self) -> usize {
        8  // input + output
    }

    fn jit_param_count(&self) -> usize {
        2  // cutoff + resonance
    }
}

/// Example: Complex node that's too hard to JIT compile
///
/// This node uses complex logic that would be difficult to translate
/// to Cranelift IR, so we DON'T implement JITCodegen.
///
/// It will automatically fall back to interpreted execution when used
/// in a JIT graph.
#[derive(Debug, Node)]
pub struct ComplexGranularSynthesis {
    #[input(value)]
    density: f32,

    #[output(stream)]
    output: f32,

    // Complex internal state
    grains: Vec<Grain>,
    random_state: RandomGenerator,
    buffer: Vec<f32>,
}

struct Grain {
    position: f32,
    velocity: f32,
    amplitude: f32,
    // ... complex state
}

struct RandomGenerator {
    // ... implementation
}

impl SignalProcessor for ComplexGranularSynthesis {
    fn process(&mut self, _sample_rate: f32, context: &mut ProcessingContext) -> f32 {
        // Complex algorithmic synthesis
        // Would be very difficult to express in Cranelift IR
        // ...

        0.0  // placeholder
    }

    // NO as_jit_codegen() implementation!
    // This node will run interpreted, which is fine for complex algorithms
}

/// Example usage
#[cfg(test)]
mod examples {
    use super::*;
    use oscen::jit::JITGraph;
    use oscen::Oscillator;

    #[test]
    fn example_custom_jit_node() {
        let mut graph = JITGraph::new(44100.0);

        // Add built-in oscillator (has JIT support)
        let osc = graph.add_node(Oscillator::sine(440.0, 0.5));

        // Add custom filter (has JIT support via our impl)
        let filter = graph.add_node(OnePoleFilter::new(0.5));

        // Connect them
        graph.connect(osc.output >> filter.input);

        // Process - both nodes will be JIT compiled!
        let output = graph.process().unwrap();

        // Output should be filtered sine wave
        assert!(output.abs() <= 1.0);
    }

    #[test]
    fn example_mixed_jit_interpreted() {
        let mut graph = JITGraph::new(44100.0);

        // JIT-compiled nodes
        let osc = graph.add_node(Oscillator::sine(440.0, 0.5));
        let filter = graph.add_node(ResonantFilter::new(0.5, 0.7));

        // Interpreted node (no JIT codegen)
        let granular = graph.add_node(ComplexGranularSynthesis::new());

        // Mix them together
        graph.connect(osc.output >> filter.input);
        graph.connect(filter.output >> granular.density);

        // Process - JIT nodes are compiled, complex node runs interpreted
        let output = graph.process().unwrap();

        // Works fine! JIT graph handles mixed execution automatically
    }
}

/// Guidelines for implementing JITCodegen
///
/// GOOD CANDIDATES for JIT:
/// - Simple math operations (add, mul, sub, div)
/// - State-space filters
/// - Oscillators with phase accumulators
/// - Envelopes with linear/exponential curves
/// - Gain/pan/mix operations
/// - Bitwise operations
///
/// AVOID JIT for:
/// - Complex algorithms (FFTs, convolution)
/// - Branching logic (if/else, loops)
/// - Dynamic memory allocation
/// - String operations
/// - File I/O
/// - External library calls
///
/// When in doubt, DON'T implement JITCodegen and let it fall back
/// to interpreted mode. You can always add JIT support later!
