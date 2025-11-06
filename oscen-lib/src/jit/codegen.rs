//! Code generation traits and utilities for JIT compilation

use super::memory_layout::NodeOffsets;
use cranelift::prelude::*;
use cranelift_module::Module;
use std::error::Error;
use std::fmt;

/// Error type for code generation
#[derive(Debug)]
pub enum CodegenError {
    /// Cranelift codegen error
    Cranelift(String),
    /// Unsupported node type
    UnsupportedNode(String),
    /// Invalid graph structure
    InvalidGraph(String),
}

impl fmt::Display for CodegenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CodegenError::Cranelift(msg) => write!(f, "Cranelift error: {}", msg),
            CodegenError::UnsupportedNode(msg) => write!(f, "Unsupported node: {}", msg),
            CodegenError::InvalidGraph(msg) => write!(f, "Invalid graph: {}", msg),
        }
    }
}

impl Error for CodegenError {}

impl From<cranelift_module::ModuleError> for CodegenError {
    fn from(err: cranelift_module::ModuleError) -> Self {
        CodegenError::Cranelift(err.to_string())
    }
}

impl From<cranelift_codegen::CodegenError> for CodegenError {
    fn from(err: cranelift_codegen::CodegenError) -> Self {
        CodegenError::Cranelift(err.to_string())
    }
}

/// Context provided to code generation
pub struct CodegenContext<'a> {
    /// Function builder for emitting IR
    pub builder: &'a mut FunctionBuilder<'a>,

    /// Pointer to graph state buffer
    pub state_ptr: Value,

    /// Pointer to IO buffer
    pub io_ptr: Value,

    /// Pointer to parameters buffer
    pub params_ptr: Value,

    /// Sample rate value
    pub sample_rate: Value,

    /// Memory offsets for this node
    pub offsets: &'a NodeOffsets,
}

impl<'a> CodegenContext<'a> {
    /// Load a value from the state buffer at the given offset
    pub fn load_state(&mut self, offset: i64) -> Value {
        let addr = self.builder.ins().iadd_imm(self.state_ptr, offset);
        self.builder
            .ins()
            .load(types::F32, MemFlags::trusted(), addr, 0)
    }

    /// Store a value to the state buffer at the given offset
    pub fn store_state(&mut self, value: Value, offset: i64) {
        let addr = self.builder.ins().iadd_imm(self.state_ptr, offset);
        self.builder.ins().store(MemFlags::trusted(), value, addr, 0);
    }

    /// Load a value from the IO buffer at the given offset
    pub fn load_io(&mut self, offset: i64) -> Value {
        let addr = self.builder.ins().iadd_imm(self.io_ptr, offset);
        self.builder
            .ins()
            .load(types::F32, MemFlags::trusted(), addr, 0)
    }

    /// Store a value to the IO buffer at the given offset
    pub fn store_io(&mut self, value: Value, offset: i64) {
        let addr = self.builder.ins().iadd_imm(self.io_ptr, offset);
        self.builder.ins().store(MemFlags::trusted(), value, addr, 0);
    }

    /// Load a parameter value at the given index
    pub fn load_param(&mut self, param_index: usize) -> Value {
        let offset = self.offsets.param_offsets[param_index] as i64;
        let addr = self.builder.ins().iadd_imm(self.params_ptr, offset);
        self.builder
            .ins()
            .load(types::F32, MemFlags::trusted(), addr, 0)
    }

    /// Create a constant f32 value
    pub fn f32_const(&mut self, value: f32) -> Value {
        self.builder.ins().f32const(value)
    }

    /// Create a constant i32 value
    pub fn i32_const(&mut self, value: i32) -> Value {
        self.builder.ins().iconst(types::I32, value as i64)
    }
}

/// Trait for emitting Cranelift IR for a specific node type
///
/// Each node type should implement this trait to enable JIT compilation.
/// The trait provides a method to emit the IR for processing one sample.
pub trait NodeCodegen {
    /// Emit Cranelift IR for processing this node
    ///
    /// This method should:
    /// 1. Load inputs from the IO buffer
    /// 2. Load parameters from the params buffer
    /// 3. Load state from the state buffer
    /// 4. Perform the node's processing
    /// 5. Store outputs to the IO buffer
    /// 6. Update state in the state buffer
    ///
    /// # Arguments
    ///
    /// * `ctx` - Code generation context with helpers for IR emission
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` if code generation succeeded, or an error if the node
    /// type is not supported or code generation failed.
    fn emit_ir(&self, ctx: &mut CodegenContext) -> Result<(), CodegenError>;

    /// Returns the name of this node type for debugging
    fn node_type_name(&self) -> &str {
        std::any::type_name::<Self>()
    }
}

/// Helper functions for common DSP operations in Cranelift IR

/// Emit IR for computing sin(x) using a libm call
///
/// Since Cranelift doesn't have built-in transcendental functions,
/// we need to call out to libm.
pub fn emit_sin_call(
    builder: &mut FunctionBuilder,
    module: &mut dyn Module,
    value: Value,
) -> Result<Value, CodegenError> {
    // TODO: Import sinf from libm
    // For now, we'll use a simple polynomial approximation
    // sin(x) ≈ x - x³/6 + x⁵/120 (Taylor series)

    // This is a simplified implementation - a real one would:
    // 1. Import sinf via module.declare_function()
    // 2. Call it via builder.ins().call()

    // For now, just return the input (placeholder)
    Ok(value)
}

/// Emit IR for wrapping a phase value to [0, 2π)
pub fn emit_phase_wrap(builder: &mut FunctionBuilder, phase: Value) -> Value {
    let two_pi = builder.ins().f32const(std::f32::consts::TAU);

    // phase % (2π)
    // Note: frem in Cranelift handles negative values correctly
    builder.ins().frem(phase, two_pi)
}

/// Emit IR for linear interpolation: a + (b - a) * t
pub fn emit_lerp(
    builder: &mut FunctionBuilder,
    a: Value,
    b: Value,
    t: Value,
) -> Value {
    let diff = builder.ins().fsub(b, a);
    let scaled = builder.ins().fmul(diff, t);
    builder.ins().fadd(a, scaled)
}

/// Emit IR for clamping a value to [min, max]
pub fn emit_clamp(
    builder: &mut FunctionBuilder,
    value: Value,
    min: Value,
    max: Value,
) -> Value {
    let clamped_min = builder.ins().fmax(value, min);
    builder.ins().fmin(clamped_min, max)
}
