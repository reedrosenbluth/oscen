//! JIT compiler implementation using Cranelift

use super::codegen::{CodegenContext, CodegenError, NodeCodegen};
use super::memory_layout::{MemoryLayout, NodeOffsets};
use super::CompiledGraphFn;
use crate::graph::{Graph, NodeKey};
use cranelift::prelude::*;
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{Linkage, Module};
use std::collections::HashMap;

/// JIT compiler for Oscen graphs
pub struct JITCompiler {
    /// Cranelift JIT module
    module: JITModule,

    /// Builder context (reused for efficiency)
    builder_context: FunctionBuilderContext,
}

impl JITCompiler {
    /// Create a new JIT compiler
    pub fn new() -> Result<Self, CodegenError> {
        let mut flag_builder = settings::builder();
        flag_builder.set("use_colocated_libcalls", "false").unwrap();
        flag_builder.set("is_pic", "false").unwrap();
        flag_builder.set("opt_level", "speed").unwrap();

        let isa_builder = cranelift_native::builder().unwrap_or_else(|msg| {
            panic!("host machine is not supported: {}", msg);
        });
        let isa = isa_builder
            .finish(settings::Flags::new(flag_builder))
            .unwrap();

        let mut builder = JITBuilder::with_isa(isa, cranelift_module::default_libcall_names());

        // Add libm functions for transcendental operations
        // builder.symbol("sinf", sinf as *const u8);
        // builder.symbol("cosf", cosf as *const u8);

        let module = JITModule::new(builder);
        let builder_context = FunctionBuilderContext::new();

        Ok(Self {
            module,
            builder_context,
        })
    }

    /// Compile a graph to machine code
    pub fn compile(
        &mut self,
        graph: &Graph,
        layout: &MemoryLayout,
        topology: &[NodeKey],
    ) -> Result<CompiledGraphFn, CodegenError> {
        // Define function signature
        let mut sig = self.module.make_signature();

        // Arguments: (state_ptr, io_ptr, params_ptr, sample_rate)
        let ptr_type = self.module.target_config().pointer_type();
        sig.params.push(AbiParam::new(ptr_type)); // state_ptr
        sig.params.push(AbiParam::new(ptr_type)); // io_ptr
        sig.params.push(AbiParam::new(ptr_type)); // params_ptr
        sig.params.push(AbiParam::new(types::F32)); // sample_rate

        // Return value: output (f32)
        sig.returns.push(AbiParam::new(types::F32));

        // Declare the function
        let func_id = self
            .module
            .declare_function("graph_process", Linkage::Export, &sig)?;

        // Create function context
        let mut ctx = self.module.make_context();
        ctx.func.signature = sig;

        // Build the function
        {
            let mut builder =
                FunctionBuilder::new(&mut ctx.func, &mut self.builder_context);

            // Create entry block
            let entry_block = builder.create_block();
            builder.append_block_params_for_function_params(entry_block);
            builder.switch_to_block(entry_block);

            // Get function parameters as Cranelift values
            let params = builder.block_params(entry_block);
            let state_ptr = params[0];
            let io_ptr = params[1];
            let params_ptr = params[2];
            let sample_rate = params[3];

            // Emit code for each node in topological order
            for &node_key in topology {
                let node_data = &graph.nodes[node_key];
                let offsets = &layout.node_offsets[&node_key];

                // Create codegen context
                let mut ctx = CodegenContext {
                    builder: &mut builder,
                    state_ptr,
                    io_ptr,
                    params_ptr,
                    sample_rate,
                    offsets,
                };

                // Try to emit specialized code for this node
                self.emit_node_code(&mut ctx, node_data)?;

                // Emit connection routing (copy outputs to connected inputs)
                self.emit_connections(
                    &mut builder,
                    io_ptr,
                    graph,
                    node_key,
                    layout,
                )?;
            }

            // Load and return the final output
            let output_offset = layout.final_output_offset as i64;
            let output_addr = builder.ins().iadd_imm(io_ptr, output_offset);
            let output = builder.ins().load(
                types::F32,
                MemFlags::trusted(),
                output_addr,
                0,
            );

            builder.ins().return_(&[output]);
            builder.finalize();
        }

        // Compile the function
        self.module.define_function(func_id, &mut ctx)?;
        self.module.clear_context(&mut ctx);

        // Finalize and get the function pointer
        self.module.finalize_definitions()?;
        let code_ptr = self.module.get_finalized_function(func_id);

        Ok(unsafe { std::mem::transmute(code_ptr) })
    }

    /// Emit code for a specific node
    fn emit_node_code(
        &self,
        ctx: &mut CodegenContext,
        node_data: &crate::graph::NodeData,
    ) -> Result<(), CodegenError> {
        // Get node type from debug representation (temporary solution)
        let debug_str = format!("{:?}", node_data.processor);

        if debug_str.contains("Gain") {
            self.emit_gain_node(ctx)
        } else if debug_str.contains("Oscillator") {
            self.emit_oscillator_node(ctx)
        } else {
            // Fallback: emit a no-op or return an error
            Err(CodegenError::UnsupportedNode(format!(
                "Node type not yet supported: {}",
                debug_str
            )))
        }
    }

    /// Emit code for a Gain node
    ///
    /// Gain: output = input * gain
    /// - IO: { input: f32, output: f32 }
    /// - Params: gain value
    fn emit_gain_node(&self, ctx: &mut CodegenContext) -> Result<(), CodegenError> {
        // Load input from IO struct (offset 0)
        let input = ctx.load_io(ctx.offsets.io_offset as i64);

        // Load gain parameter
        let gain = ctx.load_param(0);

        // Compute: output = input * gain
        let output = ctx.builder.ins().fmul(input, gain);

        // Store output to IO struct (offset 4, after input)
        ctx.store_io(output, (ctx.offsets.io_offset + 4) as i64);

        Ok(())
    }

    /// Emit code for an Oscillator node
    ///
    /// Oscillator: phase += phase_delta; output = sin(phase) * amplitude
    /// - State: { phase: f32, phase_delta: f32 }
    /// - IO: { output: f32 }
    /// - Params: frequency, amplitude
    fn emit_oscillator_node(&self, ctx: &mut CodegenContext) -> Result<(), CodegenError> {
        // Load state: phase and phase_delta
        let phase = ctx.load_state(ctx.offsets.state_offset as i64);
        let phase_delta = ctx.load_state((ctx.offsets.state_offset + 4) as i64);

        // Compute: new_phase = phase + phase_delta
        let new_phase = ctx.builder.ins().fadd(phase, phase_delta);

        // Wrap phase to [0, 2π)
        let two_pi = ctx.f32_const(std::f32::consts::TAU);
        let wrapped_phase = ctx.builder.ins().frem(new_phase, two_pi);

        // Store new phase back to state
        ctx.store_state(wrapped_phase, ctx.offsets.state_offset as i64);

        // Compute: output = sin(phase) * amplitude
        // For now, use a simple approximation instead of calling libm
        // sin(x) ≈ x for small x (this is just a placeholder)
        // TODO: Call libm sinf function
        let sin_approx = wrapped_phase; // PLACEHOLDER - not accurate!

        // Load amplitude parameter
        let amplitude = ctx.load_param(1);

        // output = sin(phase) * amplitude
        let output = ctx.builder.ins().fmul(sin_approx, amplitude);

        // Store output to IO struct
        ctx.store_io(output, ctx.offsets.io_offset as i64);

        Ok(())
    }

    /// Emit connection routing code
    ///
    /// For each output of this node, copy it to all connected inputs
    fn emit_connections(
        &self,
        builder: &mut FunctionBuilder,
        io_ptr: Value,
        graph: &Graph,
        node_key: NodeKey,
        layout: &MemoryLayout,
    ) -> Result<(), CodegenError> {
        let node_data = &graph.nodes[node_key];
        let node_offsets = &layout.node_offsets[&node_key];

        // For each output of this node
        for (output_idx, &output_key) in node_data.outputs.iter().enumerate() {
            // Get the offset of this output within the node's IO struct
            let src_field_offset = node_offsets
                .io_field_offsets
                .get(&(node_data.inputs.len() + output_idx))
                .copied()
                .unwrap_or(0);

            let src_offset = (node_offsets.io_offset + src_field_offset) as i64;

            // Load the output value
            let src_addr = builder.ins().iadd_imm(io_ptr, src_offset);
            let value = builder.ins().load(
                types::F32,
                MemFlags::trusted(),
                src_addr,
                0,
            );

            // Get all connections from this output
            if let Some(connections) = graph.connections.get(output_key) {
                for &target_input in connections {
                    // Find which node this input belongs to
                    if let Some(target_offsets) = self.find_input_offset(
                        graph,
                        layout,
                        target_input,
                    ) {
                        // Store value to target input
                        let dst_addr = builder.ins().iadd_imm(io_ptr, target_offsets as i64);
                        builder.ins().store(
                            MemFlags::trusted(),
                            value,
                            dst_addr,
                            0,
                        );
                    }
                }
            }
        }

        Ok(())
    }

    /// Find the offset of an input endpoint in the IO buffer
    fn find_input_offset(
        &self,
        graph: &Graph,
        layout: &MemoryLayout,
        input_key: crate::graph::ValueKey,
    ) -> Option<usize> {
        // Find which node owns this input
        for (node_key, node_data) in &graph.nodes {
            if let Some(input_idx) = node_data.inputs.iter().position(|&k| k == input_key) {
                // Found the node, compute offset
                let node_offsets = &layout.node_offsets[node_key];
                let field_offset = node_offsets.io_field_offsets.get(&input_idx)?;
                return Some(node_offsets.io_offset + field_offset);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compiler_creation() {
        let compiler = JITCompiler::new();
        assert!(compiler.is_ok());
    }
}
