/// Cranelift-based JIT compiler for graph execution
///
/// This compiler translates GraphIR into optimized machine code that:
/// 1. Unrolls the node processing loop
/// 2. Eliminates SlotMap lookups (uses direct indexing)
/// 3. Calls into existing node implementations
/// 4. Generates straight-line code for better CPU pipelining

use cranelift::prelude::*;
use cranelift::codegen::ir::StackSlot;
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{Linkage, Module};
use std::collections::HashMap;

use super::ir::GraphIR;
use super::runtime::GraphState;

/// JIT compiler for graphs
pub struct CraneliftJit {
    /// Cranelift JIT module
    builder_context: FunctionBuilderContext,
    ctx: codegen::Context,
    module: JITModule,
}

/// A compiled graph ready for execution
pub struct CompiledGraph {
    /// The JIT module (keeps code alive)
    _module: JITModule,

    /// Function pointer to the compiled process function
    /// Signature: fn(*mut GraphState) -> f32
    process_fn: extern "C" fn(*mut GraphState) -> f32,
}

impl CraneliftJit {
    /// Create a new JIT compiler
    pub fn new() -> Result<Self, String> {
        let mut flag_builder = settings::builder();
        flag_builder.set("use_colocated_libcalls", "false").unwrap();
        flag_builder.set("is_pic", "false").unwrap();
        flag_builder.set("opt_level", "speed").unwrap();

        let isa_builder = cranelift_native::builder()
            .map_err(|e| format!("Failed to create ISA builder: {}", e))?;
        let isa = isa_builder
            .finish(settings::Flags::new(flag_builder))
            .map_err(|e| format!("Failed to create ISA: {}", e))?;

        let builder = JITBuilder::with_isa(isa, cranelift_module::default_libcall_names());
        let module = JITModule::new(builder);

        Ok(Self {
            builder_context: FunctionBuilderContext::new(),
            ctx: module.make_context(),
            module,
        })
    }

    /// Compile a graph IR into executable machine code
    pub fn compile(&mut self, ir: &GraphIR) -> Result<CompiledGraph, String> {
        // Validate IR first
        ir.validate()?;

        // Create function signature: fn(*mut GraphState) -> f32
        let ptr_type = self.module.target_config().pointer_type();
        self.ctx.func.signature.params.push(AbiParam::new(ptr_type));
        self.ctx.func.signature.returns.push(AbiParam::new(types::F32));

        // Declare the function
        let func_id = self
            .module
            .declare_function("process_graph", Linkage::Export, &self.ctx.func.signature)
            .map_err(|e| format!("Failed to declare function: {}", e))?;

        // Build function body
        {
            let mut builder =
                FunctionBuilder::new(&mut self.ctx.func, &mut self.builder_context);
            Self::build_function_impl(&mut builder, ir, ptr_type)?;
            builder.finalize();
        }

        // Define and compile the function
        self.module
            .define_function(func_id, &mut self.ctx)
            .map_err(|e| format!("Failed to define function: {}", e))?;

        self.module.clear_context(&mut self.ctx);

        // Finalize compilation
        self.module.finalize_definitions()
            .map_err(|e| format!("Failed to finalize: {}", e))?;

        // Get function pointer
        let code_ptr = self.module.get_finalized_function(func_id);
        let process_fn: extern "C" fn(*mut GraphState) -> f32 =
            unsafe { std::mem::transmute(code_ptr) };

        Ok(CompiledGraph {
            _module: std::mem::replace(&mut self.module, Self::new()?.module),
            process_fn,
        })
    }

    /// Build the function body (static to avoid borrowing issues)
    fn build_function_impl(
        builder: &mut FunctionBuilder,
        ir: &GraphIR,
        ptr_type: Type,
    ) -> Result<(), String> {
        // Create entry block
        let entry_block = builder.create_block();
        builder.append_block_params_for_function_params(entry_block);
        builder.switch_to_block(entry_block);
        builder.seal_block(entry_block);

        // Get the GraphState pointer parameter
        let state_ptr = builder.block_params(entry_block)[0];

        // Load sample_rate from GraphState
        // offset = offsetof(GraphState, sample_rate)
        let sample_rate_offset = (9 * std::mem::size_of::<usize>()) as i32; // 9 pointer fields before sample_rate
        let sample_rate = builder.ins().load(
            types::F32,
            MemFlags::trusted(),
            state_ptr,
            sample_rate_offset,
        );

        // For each node in topology order, generate:
        // 1. Load node pointer
        // 2. Prepare ProcessingContext
        // 3. Call node.process()
        // 4. Store output for use by downstream nodes

        // We'll use stack slots to store outputs temporarily
        let mut output_slots: HashMap<usize, StackSlot> = HashMap::new();

        for &node_idx in &ir.topology_order {
            Self::emit_node_processing(builder, state_ptr, node_idx, &ir.nodes[node_idx], ptr_type, sample_rate, &mut output_slots)?;
        }

        // Return the final output
        // For now, return the last node's output
        let final_node_idx = ir.topology_order.last()
            .ok_or("Empty topology order")?;

        let final_output = if let Some(&slot) = output_slots.get(final_node_idx) {
            builder.ins().stack_load(types::F32, slot, 0)
        } else {
            // No output, return 0.0
            builder.ins().f32const(0.0)
        };

        builder.ins().return_(&[final_output]);

        Ok(())
    }

    /// Emit code for processing a single node
    fn emit_node_processing(
        builder: &mut FunctionBuilder,
        state_ptr: Value,
        node_idx: usize,
        _node_ir: &super::ir::NodeIR,
        ptr_type: Type,
        _sample_rate: Value,
        output_slots: &mut HashMap<usize, StackSlot>,
    ) -> Result<(), String> {
        // TODO: This is the core of the JIT compilation
        // For now, this is a simplified version that will be expanded

        // Load the node pointer from state.nodes[node_idx]
        // nodes is *mut *mut ()
        let nodes_offset = 0i32; // nodes is first field
        let nodes_ptr = builder.ins().load(
            ptr_type,
            MemFlags::trusted(),
            state_ptr,
            nodes_offset,
        );

        // Calculate offset to this node: nodes + (node_idx * ptr_size)
        let node_offset = (node_idx * std::mem::size_of::<*mut ()>()) as i32;
        let _node_ptr = builder.ins().load(
            ptr_type,
            MemFlags::trusted(),
            nodes_ptr,
            node_offset,
        );

        // TODO: Load process function pointer from state.process_fns[node_idx]
        // TODO: Prepare ProcessingContext with inputs from previous nodes
        // TODO: Call the process function
        // TODO: Store output in a stack slot for downstream nodes

        // For now, create a stack slot for this node's output
        let output_slot = builder.create_sized_stack_slot(StackSlotData::new(
            StackSlotKind::ExplicitSlot,
            4, // size of f32
            2, // alignment (2^2 = 4 bytes)
        ));
        output_slots.insert(node_idx, output_slot);

        // Store a placeholder value (0.0) for now
        let zero = builder.ins().f32const(0.0);
        builder.ins().stack_store(zero, output_slot, 0);

        Ok(())
    }
}

impl CompiledGraph {
    /// Execute the compiled graph
    pub fn process(&self, state: &mut GraphState) -> f32 {
        (self.process_fn)(state as *mut GraphState)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_jit_creation() {
        let jit = CraneliftJit::new();
        assert!(jit.is_ok());
    }
}
