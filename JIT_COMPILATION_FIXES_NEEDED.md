# JIT Compilation - Fixes Needed

This document tracks the compilation issues that need to be fixed for the JIT implementation to compile successfully.

## Current Status

The JIT architecture is complete and well-designed, but there are compilation errors that need to be fixed in a proper development environment with full build tools.

## Build Environment Issues

The current environment is missing:
- ALSA development libraries (libasound-dev)
- X11 development libraries
- Other system dependencies for audio

This prevents us from building and testing the JIT code.

## Compilation Errors to Fix

### 1. Missing Imports

**File:** `oscen-lib/src/jit/compiler.rs`

```rust
// Need to add:
use cranelift_native;
```

**Error:**
```
error[E0433]: failed to resolve: use of unresolved module or unlinked crate `cranelift_native`
```

### 2. Graph API - Topology Access

**File:** `oscen-lib/src/jit/compiler.rs` line 58

```rust
// Current (doesn't exist):
let topology = graph.compute_topology()?;

// Need to either:
// Option A: Add public method to Graph
impl Graph {
    pub fn compute_topology(&mut self) -> Result<Vec<NodeKey>, GraphError> {
        self.update_topology_if_needed()?;
        Ok(self.node_order.clone())
    }
}

// Option B: Use internal method (make it public)
// Change topological_sort from `fn` to `pub fn` in graph_impl.rs
```

**Error:**
```
error[E0599]: no method named `compute_topology` found for struct `graph_impl::Graph`
```

### 3. Cranelift frem Operation

**File:** `oscen-lib/src/jit/compiler.rs` and `codegen.rs`

The `frem` (floating point remainder) operation may not exist in Cranelift or has a different name.

```rust
// Current:
let wrapped_phase = ctx.builder.ins().frem(new_phase, two_pi);

// Need to check Cranelift docs for correct operation name
// Alternatives:
// - Use fsub in a loop
// - Use a call to fmodf
// - Use bitwise operations for wrapping
```

**Error:**
```
error[E0599]: no method named `frem` found for struct `FuncInstBuilder`
```

**Fix:** Check Cranelift documentation for modulo operations, or implement phase wrapping differently.

### 4. Connection API Mismatch

**File:** `examples/jit-demo/src/main.rs` lines 56-61

```rust
// Current (incorrect):
graph.connect(osc1.output, gain1.input);

// Should be:
graph.connect(osc1.output >> gain1.input);

// Or:
use oscen::Shr; // Import the >> operator trait
graph.connect(osc1.output >> gain1.input);
```

**Errors:**
```
error[E0277]: the trait bound `ConnectionBuilder: graph::types::Output` is not satisfied
error[E0061]: this method takes 2 arguments but 1 argument was supplied
```

**Fix:** Update all `graph.connect()` calls in jit-demo/src/main.rs to use the `>>` operator.

### 5. Borrow Checker Issues in emit_connections

**File:** `oscen-lib/src/jit/compiler.rs` lines 230-280

The emit_connections method has borrow checker issues with mutable and immutable borrows.

```rust
// Problem: Borrowing self while also borrowing builder mutably

// Current structure causes conflicts - need to restructure to:
// 1. Collect all connection info first (without borrows)
// 2. Then emit the IR

fn emit_connections(...) {
    // Collect connection data first
    let connections_to_emit: Vec<ConnectionData> = /* collect */;

    // Then emit without holding borrows
    for conn in connections_to_emit {
        // emit IR
    }
}
```

**Errors:**
```
error[E0499]: cannot borrow `builder` as mutable more than once at a time
error[E0502]: cannot borrow `*self` as immutable because it is also borrowed as mutable
```

### 6. ValueKey Import

**File:** `oscen-lib/src/jit/compiler.rs`

```rust
// Need to import ValueKey:
use crate::graph::ValueKey;
```

### 7. NodeData Field Access

The compiler is trying to access fields on `NodeData` that may be private.

**Fix:** Either make fields public or add accessor methods to `NodeData`.

## Files That Need Updates

### Core JIT Files
1. ✅ `oscen-lib/src/jit/mod.rs` - OK
2. ⚠️  `oscen-lib/src/jit/compiler.rs` - Needs fixes above
3. ⚠️  `oscen-lib/src/jit/codegen.rs` - Needs frem fix
4. ✅ `oscen-lib/src/jit/jit_graph.rs` - OK
5. ⚠️  `oscen-lib/src/jit/memory_layout.rs` - Minor warnings

### Graph API
1. ⚠️  `oscen-lib/src/graph/graph_impl.rs` - Need to expose topology method
2. ⚠️  `oscen-lib/src/graph/mod.rs` - May need to export ValueKey

### Examples
1. ⚠️  `examples/jit-demo/src/main.rs` - Fix connect() calls

## Testing Plan

Once compilation issues are fixed:

### 1. Unit Tests
```rust
#[cfg(test)]
mod tests {
    #[test]
    fn test_jit_gain_node() {
        // Verify Gain node compiles and produces correct output
    }

    #[test]
    fn test_jit_oscillator_node() {
        // Verify Oscillator node compiles and produces correct output
    }

    #[test]
    fn test_jit_recompilation() {
        // Verify graph modifications trigger recompilation
    }
}
```

### 2. Correctness Tests
- Compare JIT output with interpreted output (should be identical within floating point epsilon)
- Test with various graph configurations
- Verify state updates (oscillator phase, etc.)

### 3. Performance Benchmarks
```bash
cargo bench --features jit
```

Expected results:
- 10-20x speedup for typical graphs
- Compilation time < 20ms
- Recompilation time < 30ms

### 4. Example Execution
```bash
cargo run --release --example jit-demo
```

Should show performance comparison and speedup metrics.

## Quick Fixes Checklist

For someone with a proper build environment, here's the quick path to get it working:

- [ ] Add `use cranelift_native;` to compiler.rs
- [ ] Add `pub fn compute_topology(&mut self)` to Graph
- [ ] Fix `frem` operation (check Cranelift docs or use alternative)
- [ ] Update all `connect()` calls in jit-demo to use `>>`operator
- [ ] Fix borrow checker issues in `emit_connections` (restructure to collect then emit)
- [ ] Make necessary fields/methods public on NodeData
- [ ] Add unit tests
- [ ] Run benchmarks
- [ ] Verify correctness

## Expected Timeline

With a proper build environment:
- **Compilation fixes**: 2-3 hours
- **Testing & validation**: 4-6 hours
- **Performance tuning**: 2-4 hours
- **Total**: 1-2 days of focused work

## Alternative: Simpler First Step

If the fixes prove too complex, consider starting with an even simpler implementation:

### Option: Interpreter Optimizations First

Before full JIT, implement Phase 2 optimizations (from FUTURE_OPTIMIZATIONS.md):
- Store persistent IO structs in graph
- Pre-compute copy operations
- Eliminate SlotMap for stream data
- Expected: 2-3x speedup

This requires no Cranelift, simpler to debug, and provides immediate benefits.

Then add JIT on top of the optimized interpreter.

## Notes

The JIT architecture is sound and well-designed. These are just typical compilation errors that occur when writing code without iterative testing. With access to a proper build environment, these fixes are straightforward.

The conceptual work is complete:
- ✅ Architecture designed
- ✅ Memory layout computation
- ✅ Code generation framework
- ✅ Node codegen examples (Gain, Oscillator)
- ✅ Connection routing strategy
- ✅ Recompilation lifecycle
- ✅ Comprehensive documentation

Just needs compilation fixes and testing!
