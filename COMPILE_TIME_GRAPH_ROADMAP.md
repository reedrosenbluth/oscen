# Compile-Time Optimized Graph Implementation Roadmap

**Status**: Ready to implement (planning complete)
**Expected Performance**: 21.5x speedup (140ns → 6.5ns per sample)
**Estimated Code Changes**: 300-500 lines in oscen-macros/src/graph_macro/codegen.rs

## Executive Summary

This document provides a complete implementation roadmap for modifying the `graph!` macro to generate compile-time optimized structures with concrete node fields and persistent IO structs, eliminating the runtime Graph overhead.

## Current State (Completed)

✅ **Runtime StaticGraph removed** - The Vec-based runtime conversion has been removed from:
- `oscen-lib/src/graph/static_graph.rs` (deleted)
- `oscen-lib/src/graph/mod.rs` (StaticGraph export removed)
- `oscen-lib/benches/graph_bench.rs` (benchmark tests removed)

✅ **Investigation complete** - We have:
- Analyzed the macro structure (1093 lines in codegen.rs)
- Identified the key methods to modify
- Chosen the implementation strategy (ProcessingContext with pre-populated arrays)
- Validated that NO node changes are required

## Target Architecture

### Current Generated Code (Runtime Graph Wrapper)
```rust
pub struct SimpleGraph {
    pub graph: ::oscen::Graph,              // SlotMap-based runtime graph
    pub freq: ::oscen::ValueParam,
    pub osc: PolyBlepOscillatorEndpoints,   // Handle to endpoints
    pub filter: TptFilterEndpoints,
}

impl SignalProcessor for SimpleGraph {
    fn process(&mut self, sample_rate: f32, io: &mut dyn IOStructAccess, context: &mut ProcessingContext) {
        self.graph.process();  // Delegates to runtime graph
    }
}
```

**Problems**:
- SlotMap indirection on every access
- Endpoint lookups via keys
- Dynamic connection routing
- Result: ~140-190ns per sample

### Target Generated Code (Compile-Time Optimized)
```rust
pub struct SimpleGraph {
    sample_rate: f32,

    // Concrete node instances (fully inlineable!)
    osc: PolyBlepOscillator,
    filter: TptFilter,

    // Persistent IO structs (allocated once)
    osc_io: PolyBlepOscillatorIO,
    filter_io: TptFilterIO,

    // External inputs/outputs
    pub freq: ValueParam,
}

impl SimpleGraph {
    #[inline]
    pub fn process(&mut self) -> f32 {
        // Prepare contexts (minimal overhead)
        let mut events = Vec::new();

        // Process oscillator
        let osc_values = [self.freq.get_value()];
        let mut osc_ctx = ProcessingContext::new(&osc_values, &[], &[], &mut events);
        self.osc.process(self.sample_rate, &mut self.osc_io, &mut osc_ctx);

        // Direct connection routing (compile-time known!)
        self.filter_io.input = self.osc_io.output;

        // Process filter
        let filter_values = [];
        let mut filter_ctx = ProcessingContext::new(&filter_values, &[], &[], &mut events);
        self.filter.process(self.sample_rate, &mut self.filter_io, &mut filter_ctx);

        // Return final output
        self.filter_io.output
    }
}
```

**Benefits**:
- Zero indirection - direct field access
- Fully inlineable with #[inline]
- Connections resolved at compile time
- Result: ~6-7ns per sample (21.5x faster!)

## Implementation Plan

### Phase 1: Modify Struct Generation

**File**: `oscen-macros/src/graph_macro/codegen.rs`
**Method**: `generate_module_struct()` (line 949)

#### Current Code (lines 950-987):
```rust
let mut fields = vec![quote! { pub graph: ::oscen::Graph }];

// ... add input/output fields ...

// Add node handle fields
for node in &self.nodes {
    let field_name = &node.name;
    if let Some(node_type) = &node.node_type {
        let endpoints_type = Self::construct_endpoints_type(node_type);
        fields.push(quote! { pub #field_name: #endpoints_type });
    }
}
```

#### Target Code:
```rust
let mut fields = vec![quote! { sample_rate: f32 }];

// ... add input/output fields (keep as-is) ...

// Add concrete node fields + IO struct fields
for node in &self.nodes {
    let field_name = &node.name;
    if let Some(node_type) = &node.node_type {
        // Add node instance field
        fields.push(quote! { #field_name: #node_type });

        // Add IO struct field
        let io_type = Self::construct_io_type(node_type);
        let io_field_name = syn::Ident::new(
            &format!("{}_io", field_name),
            field_name.span()
        );
        fields.push(quote! { #io_field_name: #io_type });
    }
}
```

#### New Helper Method Needed:
```rust
/// Construct IO struct type from node type
/// Example: Oscillator -> OscillatorIO
fn construct_io_type(node_type: &syn::Path) -> TokenStream {
    if let Some(last_segment) = node_type.segments.last() {
        let node_name = &last_segment.ident;
        let io_name = syn::Ident::new(
            &format!("{}IO", node_name),
            node_name.span()
        );
        quote! { ::oscen::#io_name }
    } else {
        // Fallback for complex types
        quote! { ::oscen::DynamicIO }
    }
}
```

### Phase 2: Update Constructor

**Method**: `generate_module_struct()` continued (lines 1006-1020)

#### Current Code:
```rust
pub fn new(sample_rate: f32) -> Self {
    let mut graph = ::oscen::Graph::new(sample_rate);
    #input_params
    #node_creation
    #connections
    Self { #struct_init }
}
```

#### Target Code:
```rust
pub fn new(sample_rate: f32) -> Self {
    #input_params  // Keep: creates ValueParam/EventParam fields

    Self {
        sample_rate,
        #node_instances,   // NEW: direct node construction
        #io_instances,     // NEW: IO struct initialization
        #struct_init       // Keep: input/output field init
    }
}
```

#### Modify Helper Methods:

**A. Update `generate_node_creation()` (line ~400)**

Current generates:
```rust
let osc = graph.add_node(PolyBlepOscillator::saw(440.0, 0.6));
```

Should generate:
```rust
osc: PolyBlepOscillator::saw(440.0, 0.6),
osc_io: PolyBlepOscillatorIO::default(),
```

**B. Remove `generate_connections()` from constructor**
- Connections will be handled in `process()` instead
- Connection analysis still needed for topology

### Phase 3: Implement Topology Analysis

**New Method**: Add to `CodegenContext` impl

```rust
/// Compute topological order of nodes for processing
fn compute_topology_order(&self) -> Result<Vec<&NodeDecl>> {
    use std::collections::{HashMap, HashSet, VecDeque};

    // Build adjacency list from connections
    let mut graph: HashMap<String, Vec<String>> = HashMap::new();
    let mut in_degree: HashMap<String, usize> = HashMap::new();

    // Initialize
    for node in &self.nodes {
        let name = node.name.to_string();
        graph.insert(name.clone(), Vec::new());
        in_degree.insert(name, 0);
    }

    // Add edges from connection analysis
    for conn in &self.connections {
        if let Some((src_node, dst_node)) = self.extract_connection_nodes(conn) {
            graph.get_mut(&src_node).unwrap().push(dst_node.clone());
            *in_degree.get_mut(&dst_node).unwrap() += 1;
        }
    }

    // Kahn's algorithm for topological sort
    let mut queue: VecDeque<String> = in_degree.iter()
        .filter(|(_, &deg)| deg == 0)
        .map(|(name, _)| name.clone())
        .collect();

    let mut sorted = Vec::new();

    while let Some(node_name) = queue.pop_front() {
        // Find node decl by name
        if let Some(node) = self.nodes.iter().find(|n| n.name.to_string() == node_name) {
            sorted.push(node);
        }

        // Reduce in-degree of neighbors
        if let Some(neighbors) = graph.get(&node_name) {
            for neighbor in neighbors {
                let deg = in_degree.get_mut(neighbor).unwrap();
                *deg -= 1;
                if *deg == 0 {
                    queue.push_back(neighbor.clone());
                }
            }
        }
    }

    // Check for cycles
    if sorted.len() != self.nodes.len() {
        return Err(Error::new(
            Span::call_site(),
            "Cycle detected in node connections"
        ));
    }

    Ok(sorted)
}

/// Extract source and destination node names from connection
fn extract_connection_nodes(&self, conn: &ConnectionExpr) -> Option<(String, String)> {
    // Parse connection like: osc.output() >> filter.input()
    // This logic already partially exists in type_check.rs
    // Need to adapt for extracting node names
    // ... implementation details ...
}
```

### Phase 4: Generate Static process() Method

**New Method**: Add to `CodegenContext` impl

```rust
/// Generate compile-time optimized process() method
fn generate_static_process(&self) -> Result<TokenStream> {
    // 1. Get nodes in topological order
    let ordered_nodes = self.compute_topology_order()?;

    // 2. Generate processing statements for each node
    let mut process_stmts = Vec::new();

    // Prepare event storage (shared across all nodes)
    process_stmts.push(quote! {
        let mut pending_events = Vec::new();
    });

    for node in ordered_nodes {
        let node_name = &node.name;
        let io_name = syn::Ident::new(
            &format!("{}_io", node_name),
            node_name.span()
        );

        // A. Generate input assignments from connections
        let input_assigns = self.generate_node_input_assignments(node)?;
        process_stmts.extend(input_assigns);

        // B. Generate value context array
        let value_array = self.generate_value_context_array(node)?;
        process_stmts.push(quote! {
            let #node_name _values = #value_array;
            let mut #node_name _ctx = ::oscen::ProcessingContext::new(
                &#node_name _values,
                &[],
                &[],
                &mut pending_events
            );
        });

        // C. Generate process() call
        process_stmts.push(quote! {
            self.#node_name.process(
                self.sample_rate,
                &mut self.#io_name,
                &mut #node_name _ctx
            );
        });
    }

    // 3. Determine final output
    let output_expr = self.generate_output_expr()?;

    Ok(quote! {
        #[inline]
        pub fn process(&mut self) -> f32 {
            #(#process_stmts)*
            #output_expr
        }
    })
}

/// Generate stream input assignments for a node
fn generate_node_input_assignments(&self, node: &NodeDecl) -> Result<Vec<TokenStream>> {
    let mut assignments = Vec::new();

    // Find connections targeting this node
    for conn in &self.connections {
        if let Some((src_node, src_field, dst_node, dst_field)) =
            self.parse_stream_connection(conn)
        {
            if dst_node == node.name.to_string() {
                let src_node_ident = syn::Ident::new(&src_node, Span::call_site());
                let src_io = syn::Ident::new(&format!("{}_io", src_node), Span::call_site());
                let dst_io = syn::Ident::new(&format!("{}_io", dst_node), Span::call_site());

                assignments.push(quote! {
                    self.#dst_io.#dst_field = self.#src_io.#src_field;
                });
            }
        }
    }

    Ok(assignments)
}

/// Generate value input context array for a node
fn generate_value_context_array(&self, node: &NodeDecl) -> Result<TokenStream> {
    // Find value connections targeting this node
    // Determine the order of value inputs
    // Generate array like: [self.freq.get_value(), 0.0, ...]

    let mut values = Vec::new();

    // This requires analyzing node's value inputs and their connections
    // For now, placeholder:
    Ok(quote! { [] })
}

/// Generate final output expression
fn generate_output_expr(&self) -> Result<TokenStream> {
    // Determine which node/field produces the final output
    // This could be:
    // 1. Explicitly marked output
    // 2. Last node in topology
    // 3. Node with unconnected output

    // For now, use last node's first output:
    if let Some(last_node) = self.nodes.last() {
        let io_name = syn::Ident::new(
            &format!("{}_io", last_node.name),
            last_node.name.span()
        );
        Ok(quote! { self.#io_name.output })
    } else {
        Ok(quote! { 0.0 })
    }
}
```

### Phase 5: Integrate New Code Generation

**Method**: `generate_module_struct()` final integration

Update the returned TokenStream:

```rust
Ok(quote! {
    #[allow(dead_code)]
    #[derive(Debug)]
    pub struct #name {
        #(#fields),*
    }

    impl #name {
        #[allow(unused_variables, unused_mut)]
        pub fn new(sample_rate: f32) -> Self {
            #input_params
            Self {
                sample_rate,
                #(#node_instances),*
                #(#io_instances),*
                #(#other_init),*
            }
        }

        // NEW: Static process method replaces SignalProcessor impl
        #static_process_method
    }

    // Keep endpoints struct for compatibility
    #endpoints_struct

    // Remove or modify: SignalProcessor impl
    // (now process() is directly on the struct)
})
```

## Testing Strategy

### 1. Existing Tests
First verify existing `graph!` macro tests still pass (if any exist).

### 2. Simple Test Case
Create minimal test in `oscen-lib/perf/profile_graph.rs`:

```rust
graph! {
    name: TestSimple;

    node osc = PolyBlepOscillator::saw(440.0, 0.6);
}
```

Expected generated code:
```rust
pub struct TestSimple {
    sample_rate: f32,
    osc: PolyBlepOscillator,
    osc_io: PolyBlepOscillatorIO,
}

impl TestSimple {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate,
            osc: PolyBlepOscillator::saw(440.0, 0.6),
            osc_io: PolyBlepOscillatorIO::default(),
        }
    }

    #[inline]
    pub fn process(&mut self) -> f32 {
        let mut pending_events = Vec::new();
        let osc_values = [];
        let mut osc_ctx = ProcessingContext::new(&osc_values, &[], &[], &mut pending_events);
        self.osc.process(self.sample_rate, &mut self.osc_io, &mut osc_ctx);
        self.osc_io.output
    }
}
```

### 3. Connection Test Case
```rust
graph! {
    name: TestConnection;

    node osc = PolyBlepOscillator::saw(440.0, 0.6);
    node filter = TptFilter::new(1000.0, 0.707);

    osc.output() >> filter.input();
}
```

Verify connection routing is generated correctly.

### 4. Value Input Test Case
```rust
graph! {
    name: TestValueInput;

    input value freq = 440.0;

    node osc = PolyBlepOscillator::saw(440.0, 0.6);

    freq >> osc.frequency();
}
```

Verify value context array is populated correctly.

### 5. Benchmark Test
Use existing `profile_graph.rs` Voice/PolySynth examples to measure:
- Compilation time (should be same)
- Runtime performance (should be 21.5x faster)

Expected results:
- Baseline (runtime Graph): ~140ns per sample
- Target (compile-time): ~6.5ns per sample

## Implementation Checklist

### Code Changes
- [ ] Add `construct_io_type()` helper method
- [ ] Modify `generate_module_struct()` field generation
- [ ] Update `generate_node_creation()` for direct initialization
- [ ] Add `compute_topology_order()` method
- [ ] Add `extract_connection_nodes()` method
- [ ] Add `generate_static_process()` method
- [ ] Add `generate_node_input_assignments()` method
- [ ] Add `generate_value_context_array()` method
- [ ] Add `generate_output_expr()` method
- [ ] Update `generate_module_struct()` TokenStream integration

### Testing
- [ ] Compile simple test case
- [ ] Verify generated code structure
- [ ] Test connection routing
- [ ] Test value input handling
- [ ] Run full benchmark suite
- [ ] Compare with hand-written reference

### Documentation
- [ ] Update graph! macro documentation
- [ ] Add performance comparison to README
- [ ] Document any limitations or edge cases

## Known Edge Cases & Limitations

### 1. Array Nodes
Current macro supports arrays of nodes:
```rust
node oscs[8] = PolyBlepOscillator::saw(440.0, 0.6);
```

This needs special handling in the new code generation:
```rust
oscs: [PolyBlepOscillator; 8],
oscs_io: [PolyBlepOscillatorIO; 8],
```

### 2. Helper Nodes (transform, combine)
The macro currently generates helper nodes for operations like:
```rust
let mixed = osc1.output() + osc2.output();
```

These create FunctionNode instances. Need to determine:
- Should these be compiled inline?
- Or keep as FunctionNode fields?

Recommendation: Keep as concrete FunctionNode fields for now.

### 3. Feedback Connections
Nodes that allow feedback will need special handling in topology sort:
- Mark as "feedback nodes"
- Process in order but with delayed input

### 4. External Inputs/Outputs
Graph inputs/outputs need to remain accessible:
```rust
input value freq = 440.0;   // Keep as: pub freq: ValueParam
output stream out;          // How to expose?
```

Outputs might need to be stored in the struct or returned from process().

## Performance Expectations

### Baseline (Current Runtime Graph)
- Simple (1 osc): 45ns
- Medium (2 osc + filter + env): 193ns
- Complex (5 osc + 2 filters + 2 env + delay): 511ns

### Target (Compile-Time Optimized)
Based on the reference implementation achieving 21.5x speedup:
- Simple: ~2-3ns (< measurement noise)
- Medium: ~9ns
- Complex: ~24ns

### Why This Fast?
1. **Zero indirection**: No SlotMap, no Box<dyn>, no Option checks
2. **Fully inlineable**: `#[inline]` + concrete types = LLVM can optimize aggressively
3. **Compile-time routing**: Connections known at compile time
4. **Cache-friendly**: All data in contiguous struct, no pointer chasing

## Migration Path

Since this is a breaking change to the macro output, consider:

### Option A: Replace Entirely
Just update the macro - users recompile and get faster code automatically.

**Pros**: Clean, simple
**Cons**: Breaking change

### Option B: Feature Flag
```rust
#[cfg(feature = "static-graphs")]
// New compile-time code
#[cfg(not(feature = "static-graphs"))]
// Old runtime Graph wrapper
```

**Pros**: Gradual migration
**Cons**: Maintains two code paths

**Recommendation**: Option A - the performance improvement is worth a breaking change, and the macro API doesn't change (just the output).

## Next Steps

1. **Start with Phase 1**: Modify struct field generation
2. **Test incrementally**: After each phase, verify compilation
3. **Use cargo expand**: Inspect generated code to debug issues
4. **Benchmark early**: Test performance as soon as basic case works
5. **Iterate**: Refine based on results

## Success Criteria

✅ **Compiles**: Generated code compiles without errors
✅ **Correct**: Produces same audio output as runtime Graph
✅ **Fast**: Achieves 15-25x speedup over runtime Graph
✅ **Maintainable**: Code generation logic is clear and documented

## Reference Materials

- **Original macro code**: `oscen-macros/src/graph_macro/codegen.rs`
- **Existing analysis**: Connection tracking in `type_check.rs`
- **Hand-written reference**: The document you shared showing 21.5x speedup
- **Node trait**: `oscen-lib/src/graph/traits.rs` - SignalProcessor/ProcessingNode

---

**Document Status**: Complete and ready for implementation
**Last Updated**: 2025-11-12
**Estimated Implementation Time**: 4-6 hours for experienced developer
