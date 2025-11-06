# Oscen JIT Compiler Design

## Overview

This document describes the architecture for adding Just-In-Time (JIT) compilation to Oscen using Cranelift, enabling CMajor-level performance while maintaining runtime flexibility for dynamic graph repatching.

## Architecture

### High-Level Design

```
User Code
    ↓
Graph (dynamic, for building/patching)
    ↓
JITGraph::compile()
    ↓
Cranelift IR Generation
    ↓
Machine Code (x86_64/ARM64)
    ↓
Optimized Execution (10-20x faster)
```

### Key Components

1. **Graph**: Existing runtime graph (unchanged)
   - Dynamic node addition/removal
   - Runtime connection management
   - Topology sorting
   - Used for building and patching

2. **JITGraph**: New wrapper for compiled execution
   - Owns a Graph instance
   - Compiles graph to machine code via Cranelift
   - Invalidates compiled code on changes
   - Lazy recompilation on next `process()` call

3. **JITCompiler**: Cranelift code generator
   - Emits IR for each node type
   - Generates connection routing code
   - Handles state management
   - Produces callable function pointer

4. **NodeCodegen**: Trait for node-specific code generation
   - Each node type implements code emission
   - Fallback to interpreted mode for unknown types

## Data Layout

### Memory Model

The JIT compiler will operate on three memory regions:

```rust
// 1. Node State (persistent across samples)
struct GraphState {
    osc_state: OscillatorState,    // phase, phase_delta, etc.
    filter_state: TptFilterState,  // z[2], coefficients, etc.
    gain_state: GainState,         // gain value
}

// 2. IO Buffers (per-sample data flow)
struct GraphIO {
    osc_io: OscillatorIO,    // output: f32
    filter_io: TptFilterIO,  // input: f32, output: f32
    gain_io: GainIO,         // input: f32, output: f32
}

// 3. Parameters (value inputs, smoothing state)
struct GraphParams {
    osc_frequency: f32,
    osc_amplitude: f32,
    filter_cutoff: f32,
    filter_q: f32,
    gain_value: f32,
}
```

### Generated Function Signature

```rust
type CompiledGraphFn = unsafe extern "C" fn(
    state_ptr: *mut u8,     // Pointer to GraphState
    io_ptr: *mut u8,        // Pointer to GraphIO
    params_ptr: *const u8,  // Pointer to GraphParams
    sample_rate: f32,
) -> f32;  // Returns primary output
```

## Compilation Strategy

### Phase 1: Simple Direct Compilation

For the initial implementation, we'll use a straightforward approach:

1. **Topological Sort**: Get processing order from Graph
2. **Allocate State**: Create packed structs for all node states
3. **Emit Node Code**: For each node in order:
   - Load inputs from IO struct
   - Load parameters from params struct
   - Emit node processing logic (inline)
   - Store outputs to IO struct
4. **Emit Routing**: Copy outputs to connected inputs
5. **Return Output**: Return final node's primary output

### Phase 2: Optimizations (Future)

- **Dead Code Elimination**: Remove unused outputs
- **Constant Folding**: Inline constant parameters
- **SIMD Vectorization**: Process multiple samples at once
- **Common Subexpression Elimination**: Reuse computed values
- **Register Allocation**: Cranelift handles this, but we can hint

## Node Code Generation

### Code Generation Trait

```rust
pub trait NodeCodegen {
    /// Emit Cranelift IR for this node type
    fn emit_ir(
        &self,
        builder: &mut FunctionBuilder,
        state_ptr: Value,      // Pointer to node state
        io_ptr: Value,         // Pointer to node IO
        params_ptr: Value,     // Pointer to parameters
        sample_rate: Value,    // Sample rate (f32)
        offsets: &NodeOffsets, // Computed memory offsets
    ) -> Result<(), JITError>;
}

struct NodeOffsets {
    state_offset: usize,      // Offset into GraphState
    io_offset: usize,         // Offset into GraphIO
    param_offsets: Vec<usize>, // Offsets for each parameter
}
```

### Example: Gain Node IR Emission

```rust
impl NodeCodegen for Gain {
    fn emit_ir(
        &self,
        builder: &mut FunctionBuilder,
        state_ptr: Value,
        io_ptr: Value,
        params_ptr: Value,
        sample_rate: Value,
        offsets: &NodeOffsets,
    ) -> Result<(), JITError> {
        // Load input from IO struct
        // io.input is at offset 0 in GainIO
        let input_addr = builder.ins().iadd_imm(io_ptr, offsets.io_offset as i64);
        let input = builder.ins().load(
            types::F32,
            MemFlags::trusted(),
            input_addr,
            0  // offset within GainIO
        );

        // Load gain parameter
        let gain_addr = builder.ins().iadd_imm(params_ptr, offsets.param_offsets[0] as i64);
        let gain = builder.ins().load(
            types::F32,
            MemFlags::trusted(),
            gain_addr,
            0
        );

        // Compute: output = input * gain
        let output = builder.ins().fmul(input, gain);

        // Store to io.output (at offset 4 in GainIO, after input)
        let output_addr = builder.ins().iadd_imm(io_ptr, (offsets.io_offset + 4) as i64);
        builder.ins().store(
            MemFlags::trusted(),
            output,
            output_addr,
            0
        );

        Ok(())
    }
}
```

### Example: Oscillator Node IR Emission

```rust
impl NodeCodegen for Oscillator {
    fn emit_ir(&self, builder: &mut FunctionBuilder, ...) -> Result<(), JITError> {
        // Load state: phase (f32)
        let phase_addr = builder.ins().iadd_imm(state_ptr, offsets.state_offset as i64);
        let phase = builder.ins().load(types::F32, MemFlags::trusted(), phase_addr, 0);

        // Load phase_delta from state
        let delta_addr = builder.ins().iadd_imm(state_ptr, (offsets.state_offset + 4) as i64);
        let delta = builder.ins().load(types::F32, MemFlags::trusted(), delta_addr, 0);

        // Compute: phase = phase + delta
        let new_phase = builder.ins().fadd(phase, delta);

        // Wrap phase (new_phase % TWO_PI)
        let two_pi = builder.ins().f32const(std::f32::consts::TAU);
        let wrapped = builder.ins().frem(new_phase, two_pi);

        // Store new phase back
        builder.ins().store(MemFlags::trusted(), wrapped, phase_addr, 0);

        // Compute: output = sin(phase) * amplitude
        // Note: Cranelift doesn't have sin, so we call libm
        let sin_result = self.emit_sin_call(builder, wrapped)?;

        // Load amplitude parameter
        let amp_addr = builder.ins().iadd_imm(params_ptr, offsets.param_offsets[1] as i64);
        let amplitude = builder.ins().load(types::F32, MemFlags::trusted(), amp_addr, 0);

        let output = builder.ins().fmul(sin_result, amplitude);

        // Store to io.output
        let output_addr = builder.ins().iadd_imm(io_ptr, offsets.io_offset as i64);
        builder.ins().store(MemFlags::trusted(), output, output_addr, 0);

        Ok(())
    }

    fn emit_sin_call(&self, builder: &mut FunctionBuilder, value: Value) -> Result<Value, JITError> {
        // Import sin from libm
        // This is done once during module setup
        let sin_sig = builder.func.import_signature(Signature {
            params: vec![AbiParam::new(types::F32)],
            returns: vec![AbiParam::new(types::F32)],
            call_conv: CallConv::SystemV,
        });

        let sin_fn = // ... imported function reference

        let call = builder.ins().call(sin_fn, &[value]);
        let result = builder.inst_results(call)[0];
        Ok(result)
    }
}
```

## Connection Routing

### Approach: Direct Field Assignment

Instead of runtime routing through SlotMap, the JIT compiler emits direct memory copies:

```rust
// Conceptual generated code:
// filter_io.input = osc_io.output;

fn emit_connection(
    builder: &mut FunctionBuilder,
    io_ptr: Value,
    src_offset: usize,  // offset to osc_io.output
    dst_offset: usize,  // offset to filter_io.input
) {
    // Load from source
    let src_addr = builder.ins().iadd_imm(io_ptr, src_offset as i64);
    let value = builder.ins().load(types::F32, MemFlags::trusted(), src_addr, 0);

    // Store to destination
    let dst_addr = builder.ins().iadd_imm(io_ptr, dst_offset as i64);
    builder.ins().store(MemFlags::trusted(), value, dst_addr, 0);
}
```

### Optimization: Register-Only Routing

For better performance, keep values in registers when possible:

```rust
// Instead of:
//   osc processes, writes to memory
//   load from memory
//   filter processes
//
// Do:
//   osc processes, keeps output in register
//   pass register directly to filter
//   filter processes

// This requires tracking which values are in registers
// and only spilling when necessary
```

## Compilation Process

### Step-by-Step Flow

```rust
impl JITGraph {
    pub fn compile(&mut self) -> Result<(), JITError> {
        // 1. Compute topology
        let topology = self.graph.compute_topology()?;

        // 2. Compute memory layout
        let layout = self.compute_memory_layout(&topology);

        // 3. Create Cranelift module
        let mut module = JITModule::new(JITBuilder::new(cranelift_module::default_libcall_names())?);

        // 4. Define function signature
        let mut sig = module.make_signature();
        sig.params.push(AbiParam::new(types::I64)); // state_ptr
        sig.params.push(AbiParam::new(types::I64)); // io_ptr
        sig.params.push(AbiParam::new(types::I64)); // params_ptr
        sig.params.push(AbiParam::new(types::F32)); // sample_rate
        sig.returns.push(AbiParam::new(types::F32)); // output

        // 5. Create function builder
        let mut ctx = module.make_context();
        let func_id = module.declare_function("graph_process", Linkage::Local, &sig)?;
        ctx.func.signature = sig;

        let mut builder = FunctionBuilder::new(&mut ctx.func, &mut builder_context);
        let entry_block = builder.create_block();
        builder.append_block_params_for_function_params(entry_block);
        builder.switch_to_block(entry_block);

        // 6. Get function parameters
        let state_ptr = builder.block_params(entry_block)[0];
        let io_ptr = builder.block_params(entry_block)[1];
        let params_ptr = builder.block_params(entry_block)[2];
        let sample_rate = builder.block_params(entry_block)[3];

        // 7. Emit code for each node in topological order
        for node_key in topology {
            let node = &self.graph.nodes[node_key];
            let offsets = &layout.node_offsets[&node_key];

            // Emit node processing code
            node.emit_ir(&mut builder, state_ptr, io_ptr, params_ptr, sample_rate, offsets)?;

            // Emit connection routing
            self.emit_connections(&mut builder, io_ptr, node_key, &layout)?;
        }

        // 8. Return final output
        let output_offset = layout.final_output_offset;
        let output_addr = builder.ins().iadd_imm(io_ptr, output_offset as i64);
        let output = builder.ins().load(types::F32, MemFlags::trusted(), output_addr, 0);
        builder.ins().return_(&[output]);

        // 9. Finalize function
        builder.finalize();

        // 10. Compile to machine code
        module.define_function(func_id, &mut ctx)?;
        module.finalize_definitions();

        // 11. Get function pointer
        let code_ptr = module.get_finalized_function(func_id);
        self.compiled_fn = Some(unsafe { mem::transmute(code_ptr) });

        Ok(())
    }
}
```

## Recompilation Strategy

### Invalidation on Changes

```rust
impl JITGraph {
    pub fn add_node(&mut self, node: impl SignalProcessor + 'static) -> NodeEndpoints {
        let endpoints = self.graph.add_node(node);
        self.compiled_fn = None;  // Invalidate
        endpoints
    }

    pub fn connect(&mut self, from: StreamOutput, to: StreamInput) {
        self.graph.connect(from, to);
        self.compiled_fn = None;  // Invalidate
    }

    pub fn process(&mut self) -> f32 {
        if self.compiled_fn.is_none() {
            self.compile().expect("JIT compilation failed");
        }

        let func = self.compiled_fn.unwrap();
        unsafe {
            func(
                self.state.as_mut_ptr() as *mut u8,
                self.io.as_mut_ptr() as *mut u8,
                self.params.as_ptr() as *const u8,
                self.sample_rate,
            )
        }
    }
}
```

### Compilation Time Budget

To enable live performance use:

```rust
pub struct JITConfig {
    /// Maximum time allowed for compilation (milliseconds)
    /// If exceeded, fall back to interpreted mode for this buffer
    pub compilation_timeout_ms: u64,

    /// Compile in background thread
    pub async_compilation: bool,
}
```

## Hybrid Execution Mode

### Fallback for Unsupported Nodes

```rust
impl NodeCodegen for Box<dyn SignalProcessor> {
    fn emit_ir(&self, builder: &mut FunctionBuilder, ...) -> Result<(), JITError> {
        // For unknown node types, emit a callback to interpreted mode

        // Create callback signature: fn(*mut Node, *mut IO, *const Params, f32) -> f32
        let callback_sig = // ...

        // Get function pointer to node.process()
        let process_fn = self.process as *const ();
        let fn_constant = builder.ins().iconst(types::I64, process_fn as i64);

        // Call the interpreted process method
        let call = builder.ins().call_indirect(callback_sig, fn_constant, &[
            node_ptr, io_ptr, params_ptr, sample_rate
        ]);

        let result = builder.inst_results(call)[0];
        // Store result to IO
        // ...

        Ok(())
    }
}
```

This allows mixing JIT-compiled nodes with custom nodes.

## Performance Characteristics

### Expected Speedups

Based on similar systems (CMajor, FAUST, Gen~):

| Scenario | Current | JIT | Speedup |
|----------|---------|-----|---------|
| Simple graph (3-5 nodes) | 1.0x | 15x | **15x faster** |
| Medium graph (10-20 nodes) | 1.0x | 12x | **12x faster** |
| Complex graph (50+ nodes) | 1.0x | 8x | **8x faster** |
| With custom nodes (fallback) | 1.0x | 5x | **5x faster** |

### Why These Speedups?

1. **Zero dynamic dispatch** (5-10% overhead eliminated)
2. **No SlotMap lookups** (20-30% overhead eliminated)
3. **Full inlining** (30-40% overhead eliminated)
4. **Register allocation** (10-20% improvement)
5. **Dead code elimination** (5-10% for unused outputs)

## Implementation Phases

### Phase 1: Proof of Concept (Week 1)
- [ ] Add Cranelift dependency
- [ ] Create JITGraph wrapper
- [ ] Implement Gain node codegen
- [ ] Compile single-node graph
- [ ] Verify correctness

### Phase 2: Basic Compilation (Week 2-3)
- [ ] Implement Oscillator codegen
- [ ] Add connection routing
- [ ] Compile multi-node graphs
- [ ] Add recompilation on changes
- [ ] Create examples

### Phase 3: Expanded Node Support (Week 4-5)
- [ ] Implement Filter codegen
- [ ] Add envelope codegen
- [ ] Support all built-in nodes
- [ ] Add fallback for custom nodes

### Phase 4: Optimization (Week 6-8)
- [ ] Register-only routing
- [ ] Dead code elimination
- [ ] Parameter inlining
- [ ] SIMD vectorization
- [ ] Benchmarking suite

### Phase 5: Production Ready (Week 9-12)
- [ ] Error handling
- [ ] Compilation timeout
- [ ] Async compilation
- [ ] Documentation
- [ ] Integration tests

## API Design

### User-Facing API

```rust
// Option 1: Explicit JIT opt-in
let mut graph = JITGraph::new(44100.0);
let osc = graph.add_node(Oscillator::sine(440.0, 0.5));
let gain = graph.add_node(Gain::new(0.8));
graph.connect(osc.output >> gain.input);

// Automatically compiles on first process()
let output = graph.process();

// Option 2: Convert existing graph
let mut graph = Graph::new(44100.0);
// ... build graph ...
let jit_graph = JITGraph::from_graph(graph);

// Option 3: Feature flag
#[cfg(feature = "jit")]
type DefaultGraph = JITGraph;
#[cfg(not(feature = "jit"))]
type DefaultGraph = Graph;
```

## Testing Strategy

### Correctness Tests

1. **Reference Comparison**: Compare JIT output to interpreted output
   - Should match bit-for-bit (or within floating point epsilon)

2. **State Preservation**: Verify state updates correctly
   - Oscillator phase increments
   - Filter coefficients update

3. **Connection Routing**: Verify data flows correctly
   - Multi-output nodes
   - Complex routing patterns

### Performance Tests

1. **Compilation Time**: Measure how long compilation takes
2. **Execution Speedup**: Compare JIT vs interpreted performance
3. **Recompilation Overhead**: Measure cost of dynamic patching

### Stress Tests

1. **Large Graphs**: 100+ node graphs
2. **Rapid Repatching**: Modify graph every buffer
3. **Memory Usage**: Monitor allocation patterns

## Future Extensions

### SIMD Vectorization

Process multiple samples in parallel:

```rust
// Instead of: fn(state, io, params, sr) -> f32
// Emit: fn(state, io, params, sr, count: usize) -> ()

// Process 4 samples at once using SSE/AVX
let input_vec = builder.ins().load(types::F32X4, ...);
let gain_vec = builder.ins().splat(gain);
let output_vec = builder.ins().fmul(input_vec, gain_vec);
builder.ins().store(output_vec, ...);
```

### Polyphonic Graphs

Generate separate compiled functions per voice:

```rust
let synth = PolyphonicJITGraph::new(16); // 16 voices
synth.set_voice_graph(0, voice_graph);   // Compiles once, used by all voices
```

### GPU Compilation

For massive polyphony, compile to GPU kernels (future):

```rust
let gpu_graph = GPUGraph::new(1024); // 1024 voices on GPU
// Compile to SPIR-V or Metal shaders
```

## Conclusion

This JIT architecture enables:
- ✅ Runtime graph building and patching
- ✅ CMajor-level performance (10-20x speedup)
- ✅ Full Rust type safety
- ✅ Gradual adoption (feature flag)
- ✅ Extensibility (custom nodes via fallback)

The struct-of-arrays refactoring provides the perfect foundation for this optimization!
