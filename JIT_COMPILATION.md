# JIT Compilation for Oscen Graphs

This document describes the JIT (Just-In-Time) compilation system for Oscen, which allows graphs to be dynamically compiled to optimized machine code for significantly faster execution.

## Overview

The JIT system uses **Cranelift** to compile graph execution logic to native machine code. The key innovation is that **node authors don't need to change anything** - we JIT compile the graph routing and execution loop, but still call into existing node implementations.

### Performance Benefits

Based on similar systems and eliminating the interpreted overhead:
- **3-5x faster execution** from unrolling loops and eliminating SlotMap lookups
- **Faster compilation** compared to LLVM (milliseconds vs seconds) using Cranelift
- **Dynamic reconfiguration** - recompile when graph topology changes

## Architecture

### Three-Layer Design

```
┌─────────────────────────────────────┐
│   Interpreted Graph (Graph)          │  ← Current system, always works
│   - SlotMap-based                    │
│   - Dynamic dispatch                 │
│   - Flexible, slower                 │
└──────────────┬──────────────────────┘
               │
               ▼ to_ir()
┌─────────────────────────────────────┐
│   Intermediate Rep (GraphIR)         │  ← Topology + connections
│   - Node topology                    │
│   - Connection routing               │
│   - Metadata                         │
└──────────────┬──────────────────────┘
               │
               ▼ compile()
┌─────────────────────────────────────┐
│   Compiled Graph (CompiledGraph)     │  ← Native machine code
│   - Unrolled loops                   │
│   - Direct calls                     │
│   - Fast, inflexible                 │
└─────────────────────────────────────┘
```

### What Gets JIT Compiled

We JIT compile the **graph execution loop**, not individual nodes:

**Current interpreted execution:**
```rust
// graph_impl.rs::process()
for node_key in node_order {                   // ← Interpreted loop
    let node = nodes.get_mut(node_key);       // ← SlotMap lookup
    for input in node.inputs {                 // ← Loop
        let value = endpoints.get(input);     // ← SlotMap lookup
        input_values[i] = value;
    }
    let output = node.process(...);           // ← Virtual call
    for &target in connections.get(output) {  // ← Loop + SlotMap
        endpoints.get_mut(target).set(output);
    }
}
```

**JIT compiled version:**
```asm
; Unrolled, direct memory accesses, no loops!
; Load node 0 function pointer
mov rax, [rdi + 8]           ; state.process_fns[0]
mov rdi, [rdi]               ; state.nodes[0]
call rax                     ; Call node.process()
mov [rsp + 0], rax           ; Store output

; Load node 1 function pointer
mov rax, [rdi + 16]          ; state.process_fns[1]
mov rdi, [rdi + 8]           ; state.nodes[1]
; ... input already in register from previous output
call rax
mov [rsp + 4], rax

; Return final output
mov rax, [rsp + 4]
ret
```

## Core Components

### 1. GraphIR (Intermediate Representation)

Location: `oscen-lib/src/graph/jit/ir.rs`

```rust
pub struct GraphIR {
    nodes: Vec<NodeIR>,
    connections: Vec<ConnectionIR>,
    topology_order: Vec<usize>,
    sample_rate: f32,
}
```

Captures:
- Node topology (inputs/outputs per node)
- Connection routing (which nodes feed which)
- Processing order (topologically sorted)

### 2. CraneliftJit (Compiler)

Location: `oscen-lib/src/graph/jit/compiler.rs`

```rust
pub struct CraneliftJit {
    builder_context: FunctionBuilderContext,
    ctx: codegen::Context,
    module: JITModule,
}
```

Compiles GraphIR → machine code:
1. Generate Cranelift IR for graph execution
2. Optimize with Cranelift's optimizer
3. Compile to native code
4. Return function pointer

### 3. CompiledGraph (Executable)

```rust
pub struct CompiledGraph {
    _module: JITModule,  // Keeps code alive
    process_fn: extern "C" fn(*mut GraphState) -> f32,
}
```

Holds:
- Compiled machine code
- Function pointer for execution

### 4. GraphState (Runtime Data)

Passed to JIT code at runtime:
```rust
#[repr(C)]
pub struct GraphState {
    nodes: *mut *mut (),           // Node instances
    process_fns: *const ProcessFn, // Function pointers
    endpoints: *mut (),            // Endpoint storage
    // ... temp buffers, metadata
    sample_rate: f32,
    node_count: u32,
}
```

## Usage Example

### Basic JIT Compilation

```rust
use oscen::graph::{Graph, jit::CraneliftJit};
use oscen::oscillators::Oscillator;
use oscen::filters::IirLowpass;
use oscen::gain::Gain;

// Build graph normally
let mut graph = Graph::new(44100.0);
let osc = graph.add_node(Oscillator::sine(440.0, 0.5));
let filter = graph.add_node(IirLowpass::new(1000.0, 0.7));
let gain = graph.add_node(Gain::new(0.8));

graph.connect(osc.output, filter.input);
graph.connect(filter.output, gain.input);

// Extract IR
let ir = graph.to_ir().expect("Failed to extract IR");

// Compile to machine code
let mut jit = CraneliftJit::new().expect("Failed to create JIT");
let compiled = jit.compile(&ir).expect("Compilation failed");

// Execute compiled graph
// (Runtime state management to be implemented)
```

### Dynamic Reconfiguration

```rust
// Change parameter (no recompilation needed!)
graph.set_value(filter.cutoff, 2000.0);

// Change topology (requires recompilation)
let reverb = graph.add_node(Reverb::new());
graph.connect(gain.output, reverb.input);

// Recompile
let ir = graph.to_ir()?;
let compiled = jit.compile(&ir)?;

// Hot-swap: compile in background, swap atomically
// (To be implemented)
```

## Implementation Status

### ✅ Completed

1. **GraphIR module** - Intermediate representation
2. **Graph::to_ir()** - Extract topology from graph
3. **CraneliftJit** - JIT compiler infrastructure
4. **Cranelift dependencies** - Build system integration
5. **Basic code generation** - Skeleton implementation

### 🚧 In Progress

1. **Code generation** - Full node processing loop
   - Load function pointers
   - Prepare ProcessingContext
   - Call node.process()
   - Route outputs

### 📋 TODO

1. **Runtime state management**
   - Build GraphState from Graph
   - Extract function pointers from trait objects
   - Manage memory lifetimes

2. **JitGraph wrapper**
   - Automatic compilation on topology changes
   - Fallback to interpreted mode
   - Hot-swapping compiled graphs

3. **Optimizations**
   - Direct struct field copying (eliminate ProcessingContext)
   - Inline constant parameters
   - SIMD vectorization

4. **Testing & Benchmarks**
   - Unit tests for IR extraction
   - Integration tests for compilation
   - Benchmarks vs interpreted execution

## Design Decisions

### Why Cranelift Over LLVM?

1. **Faster compilation** - 10-100x faster than LLVM
2. **Simpler API** - Easier to work with
3. **Better for JIT** - Designed for runtime compilation
4. **Trade-off** - ~90% of LLVM's code quality (still very fast!)

For dynamic audio graphs, fast recompilation is more important than squeezing out every last cycle.

### Why Not JIT Individual Nodes?

JIT-compiling nodes themselves would require:
1. Translating Rust → Cranelift IR (extremely complex)
2. Handling Rust stdlib calls (sin, cos, etc.)
3. Managing complex internal state

Instead, we:
- **Keep nodes as Rust code** (easy to write)
- **JIT the glue between nodes** (simple, big gains)
- **Call into nodes via function pointers** (works with trait objects)

This gives us 80% of the benefit with 20% of the complexity.

### Zero Changes for Node Authors

Users write nodes exactly as before:
```rust
#[derive(Debug, Node)]
pub struct MyNode {
    #[input(stream)]
    input: f32,

    #[output(stream)]
    output: f32,
}

impl SignalProcessor for MyNode {
    fn process(&mut self, sample_rate: f32, context: &mut ProcessingContext) -> f32 {
        let mut io = MyNodeIO {
            input: self.get_input(context),
            output: 0.0,
        };
        io.output = io.input * 2.0;
        io.output
    }
}
```

The JIT system calls this code - no changes needed!

## Future Optimizations

### Phase 1: Complete Basic JIT (Current)
- Unroll node processing loop
- Eliminate SlotMap lookups for topology
- Expected: **3-5x speedup**

### Phase 2: Direct Struct Field Copying
- Eliminate ProcessingContext overhead
- Direct memory copies between IO structs
- Expected: **2-3x additional speedup**

### Phase 3: Compile-Time Graphs
- Macro-generated static graphs
- Full cross-node inlining
- Expected: **10-20x speedup**

### Phase 4: SIMD Vectorization
- Process multiple samples at once
- Use AVX/SSE instructions
- Expected: **4-8x additional speedup**

**Total potential: 60-80x faster than current interpreted execution!**

## How It Works: Deep Dive

### IR Extraction

`Graph::to_ir()` walks the graph and extracts:

1. **Node metadata** - inputs/outputs per node
2. **Topology order** - which order to process nodes
3. **Connections** - routing between nodes
4. **Endpoint keys** - for accessing graph state

### Code Generation

`CraneliftJit::compile()` generates:

```rust
function process_graph(state: *GraphState) -> f32 {
    block0(state: ptr):
        // Load graph state pointers
        nodes = load state.nodes
        process_fns = load state.process_fns
        sample_rate = load state.sample_rate

        // For each node in topology order:
        for i in topology_order {
            // Load node pointer
            node_i = load nodes[i]

            // Load function pointer
            fn_i = load process_fns[i]

            // Prepare ProcessingContext
            // (TODO: gather inputs from previous nodes)

            // Call node.process()
            output_i = call fn_i(node_i, sample_rate, context)

            // Store for downstream nodes
            stack_store output_i
        }

        // Return final output
        return output_N
}
```

### Execution

```rust
// Setup state
let mut state = GraphState {
    nodes: &mut graph.nodes,
    process_fns: extract_fn_pointers(&graph.nodes),
    // ...
};

// Call compiled code
let output = compiled_graph.process(&mut state);
```

## Comparison with Other Systems

### vs CMajor
- **CMajor**: Compile-time only, DSL
- **Oscen**: Runtime + compile-time, Rust
- **Similarity**: Struct-of-arrays pattern

### vs FAUST
- **FAUST**: DSL, compiles to C++
- **Oscen**: Rust, JIT compilation
- **Similarity**: Optimization focus

### vs Pure Data / Max
- **PD/Max**: Fully interpreted
- **Oscen**: Interpreted + JIT
- **Advantage**: Can switch between modes

## Performance Expectations

Rough estimates based on similar systems:

| Optimization | Speedup | Total |
|--------------|---------|-------|
| Current (interpreted) | 1x | 1x |
| Basic JIT | 3-5x | 3-5x |
| Direct field copying | 2-3x | 10-15x |
| Compile-time graphs | 2-3x | 30-45x |
| SIMD vectorization | 4-8x | 120-360x |

Conservative estimate: **60-80x faster than current**

## Contributing

To extend the JIT system:

1. **Add optimizations** in `compiler.rs::emit_node_processing()`
2. **Improve IR** in `ir.rs` to capture more information
3. **Add benchmarks** to measure real-world gains
4. **Test edge cases** - delays, feedback, events

## References

- [Cranelift Documentation](https://cranelift.dev/)
- [CMajor](https://cmajor.dev/) - Inspiration for struct-of-arrays
- [FAUST](https://faust.grame.fr/) - Functional audio DSL
- [LLVM JIT Tutorial](https://llvm.org/docs/tutorial/)
