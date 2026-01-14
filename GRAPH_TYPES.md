# Graph Types in Oscen: Static vs. Dynamic

Oscen provides two distinct ways to build audio graphs using the `graph!` macro: **Compiled (Static)** and **Runtime (Dynamic)**. While they share the same syntax, they generate fundamentally different code with different trade-offs between flexibility and performance.

## 1. Compiled Graphs (Static) - Default

This is the default mode. It generates a specialized struct where every node is a concrete field. The processing logic is "baked" into a linear sequence of function calls.

### Characteristics
*   **Fixed Topology**: The structure is frozen at compile time. You cannot add/remove nodes at runtime.
*   **Zero Overhead**:
    *   No heap allocation for the graph structure.
    *   No dynamic dispatch (virtual function calls).
    *   No intermediate buffers or routing logic.
    *   Direct field access (`self.filter.input = self.osc.output`).
*   **Inlining**: The Rust compiler can aggressively inline the entire process loop, often reducing it to a few SIMD instructions.
*   **Performance**: Can be **20x-50x faster** than runtime graphs (e.g., ~5ns vs ~150ns per sample).

### Example
```rust
graph! {
    name: StaticSynth;
    // compile_time: true (default)

    input value freq = 440.0;
    output stream out;

    node osc = PolyBlepOscillator::saw(440.0, 0.6);
    connection freq -> osc.frequency();
    connection osc.output() -> out;
}
```

### Generated Code Structure (Simplified)
```rust
pub struct StaticSynth {
    // Concrete types, no boxing!
    osc: PolyBlepOscillator,
    // ... other fields ...
}

impl StaticSynth {
    #[inline(always)]
    pub fn process(&mut self) {
        // Direct data transfer
        self.osc.frequency = self.freq_param;

        // Direct function call (monomorphized)
        self.osc.process();

        // Output routing
        self.out = self.osc.output;
    }
}
```

---

## 2. Runtime Graphs (Dynamic)

Enabled by setting `compile_time: false`. It generates a wrapper around `oscen::Graph`, a dynamic data structure that manages nodes and connections at runtime.

### Characteristics
*   **Flexible**: Nodes and connections are stored in dynamic collections (SlotMaps).
*   **Reconfigurable**: You could theoretically modify the graph structure while it's running (though the macro generates a fixed setup, the underlying `Graph` allows modification).
*   **Type Erasure**: Nodes are stored as trait objects (`Box<dyn DynNode>`), involving dynamic dispatch.
*   **Overhead**: Processing involves looking up nodes, routing signals through intermediate buffers, and iterating over connections.

### Example
```rust
graph! {
    name: DynamicSynth;
    compile_time: false; // <--- Enables runtime generation

    input value freq = 440.0;
    output stream out;

    node osc = PolyBlepOscillator::saw(440.0, 0.6);
    connection freq -> osc.frequency();
    connection osc.output() -> out;
}
```

### Generated Code Structure (Simplified)
```rust
pub struct DynamicSynth {
    graph: oscen::Graph, // Holds all state dynamically
    // ... handles to endpoints ...
}

impl SignalProcessor for DynamicSynth {
    fn process(&mut self) {
        self.graph.process(); // Iterates over all nodes dynamically
    }
}
```

---

## Summary Comparison

| Feature | Compiled Graph (Default) | Runtime Graph (`compile_time: false`) |
| :--- | :--- | :--- |
| **Structure** | Struct with concrete fields | `oscen::Graph` wrapper |
| **Flexibility** | Low (fixed topology) | High (dynamic topology) |
| **Dispatch** | Static (Concrete types) | Dynamic (`dyn Trait`) |
| **Performance** | Extreme (~5ns/sample) | Good (~150ns/sample) |
| **Use Case** | VSTs, fixed synths, embedded | Modular environments, patching |
