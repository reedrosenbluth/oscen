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
    /// ID of the declared trampoline function
    trampoline_id: Option<cranelift_module::FuncId>,
    /// ID of the write_node_output helper function
    write_output_id: Option<cranelift_module::FuncId>,
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

        let mut builder = JITBuilder::with_isa(isa, cranelift_module::default_libcall_names());

        // Add the trampoline function as a symbol
        let trampoline_ptr = super::runtime::get_trampoline_ptr() as *const u8;
        builder.symbol("process_node_trampoline", trampoline_ptr);

        // Add the write_node_output helper as a symbol
        let write_output_ptr = super::runtime::write_node_output as *const u8;
        builder.symbol("write_node_output", write_output_ptr);

        let module = JITModule::new(builder);

        Ok(Self {
            builder_context: FunctionBuilderContext::new(),
            ctx: module.make_context(),
            module,
            trampoline_id: None,
            write_output_id: None,
        })
    }

    /// Compile a graph IR into executable machine code
    pub fn compile(&mut self, ir: &GraphIR) -> Result<CompiledGraph, String> {
        // Validate IR first
        ir.validate()?;

        let ptr_type = self.module.target_config().pointer_type();

        // Declare the trampoline function if not already declared
        if self.trampoline_id.is_none() {
            let mut trampoline_sig = self.module.make_signature();
            trampoline_sig.params.push(AbiParam::new(types::I64)); // node_index (usize)
            trampoline_sig.params.push(AbiParam::new(types::F32)); // sample_rate
            trampoline_sig.params.push(AbiParam::new(ptr_type));   // context_ptr
            trampoline_sig.params.push(AbiParam::new(ptr_type));   // state_ptr
            trampoline_sig.returns.push(AbiParam::new(types::F32)); // return value

            let trampoline_id = self
                .module
                .declare_function("process_node_trampoline", Linkage::Import, &trampoline_sig)
                .map_err(|e| format!("Failed to declare trampoline: {}", e))?;

            self.trampoline_id = Some(trampoline_id);
        }

        // Declare the write_node_output helper if not already declared
        if self.write_output_id.is_none() {
            let mut write_output_sig = self.module.make_signature();
            write_output_sig.params.push(AbiParam::new(ptr_type));   // state_ptr
            write_output_sig.params.push(AbiParam::new(types::I64)); // node_index (usize)
            write_output_sig.params.push(AbiParam::new(types::F32)); // output_value
            // No return value

            let write_output_id = self
                .module
                .declare_function("write_node_output", Linkage::Import, &write_output_sig)
                .map_err(|e| format!("Failed to declare write_node_output: {}", e))?;

            self.write_output_id = Some(write_output_id);
        }

        // Create function signature: fn(*mut GraphState) -> f32
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
            let trampoline_id = self.trampoline_id.unwrap();
            let write_output_id = self.write_output_id.unwrap();
            Self::build_function_impl(&mut builder, ir, ptr_type, trampoline_id, write_output_id, &mut self.module)?;
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
        trampoline_id: cranelift_module::FuncId,
        write_output_id: cranelift_module::FuncId,
        module: &mut JITModule,
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
        // GraphState has 8 pointer-sized fields before sample_rate:
        // nodes_slotmap, node_keys, endpoints, endpoint_keys, input_offsets, output_offsets, connections_offsets, connections_data
        let sample_rate_offset = (8 * std::mem::size_of::<usize>()) as i32;
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
            Self::emit_node_processing(builder, module, state_ptr, node_idx, &ir.nodes[node_idx], ptr_type, sample_rate, trampoline_id, write_output_id, &mut output_slots)?;
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
        module: &mut JITModule,
        state_ptr: Value,
        node_idx: usize,
        _node_ir: &super::ir::NodeIR,
        ptr_type: Type,
        sample_rate: Value,
        trampoline_id: cranelift_module::FuncId,
        write_output_id: cranelift_module::FuncId,
        output_slots: &mut HashMap<usize, StackSlot>,
    ) -> Result<(), String> {
        // Get a local reference to the trampoline function
        let trampoline_func_ref = module.declare_func_in_func(trampoline_id, builder.func);
        let write_output_func_ref = module.declare_func_in_func(write_output_id, builder.func);

        // Prepare arguments for trampoline call
        // Signature: fn(node_index: usize, sample_rate: f32, context: *mut (), state: *mut GraphState) -> f32

        // 1. node_index as i64 (usize on 64-bit)
        let node_idx_val = builder.ins().iconst(types::I64, node_idx as i64);

        // 2. sample_rate (already a Value)
        let sample_rate_val = sample_rate;

        // 3. context_ptr - for now, pass null (trampoline builds context itself)
        let null_context = builder.ins().iconst(ptr_type, 0);

        // 4. state_ptr (already a Value)
        let state_val = state_ptr;

        // Call the trampoline
        let call_inst = builder.ins().call(
            trampoline_func_ref,
            &[node_idx_val, sample_rate_val, null_context, state_val],
        );

        // Get the return value (f32)
        let results = builder.inst_results(call_inst);
        let output_val = results[0];

        // Write the output back to the endpoint so downstream nodes can read it
        // Signature: fn(state: *mut GraphState, node_index: usize, output_value: f32)
        builder.ins().call(
            write_output_func_ref,
            &[state_val, node_idx_val, output_val],
        );

        // Create a stack slot for this node's output
        let output_slot = builder.create_sized_stack_slot(StackSlotData::new(
            StackSlotKind::ExplicitSlot,
            4, // size of f32
            2, // alignment (2^2 = 4 bytes)
        ));
        output_slots.insert(node_idx, output_slot);

        // Store the output value
        builder.ins().stack_store(output_val, output_slot, 0);

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
