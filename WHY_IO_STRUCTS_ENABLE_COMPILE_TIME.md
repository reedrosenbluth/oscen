# Why IO Structs Enable Compile-Time Graph Generation

## The Key Insight: Type Information vs Runtime Metadata

The fundamental difference is:
- **Old pattern**: Relies on **runtime metadata** (indices, SlotMaps, type erasure)
- **New pattern**: Exposes **compile-time type information** (concrete structs, named fields)

Compile-time graph generation requires the **compiler to know**:
1. What types connect to what types
2. What field connects to what field
3. The exact memory layout of each node's I/O
4. Which outputs go where

Let's see why the old pattern couldn't provide this, and why the new pattern can.

---

## Old Pattern: Why Compile-Time Generation Was Impossible

### The Old API

```rust
trait SignalProcessor {
    fn process(&mut self, sample_rate: f32, context: &mut ProcessingContext) -> f32;
    //                                                                            ↑
    //                                                        Single f32 return - which output is this?
}

// Usage:
let output = gain.process(sample_rate, &mut context);  // Just an f32!
```

### Problem 1: Type Erasure - No Concrete I/O Types

```rust
// All nodes use the SAME context type
let gain_output = gain.process(sr, &mut context);
let filter_output = filter.process(sr, &mut context);
let osc_output = osc.process(sr, &mut context);

// At compile time, these all look identical!
// The compiler doesn't know:
// - What inputs each node expects
// - What outputs each node produces
// - How they differ from each other
```

**Why this blocks compile-time generation:**

If we try to generate a compile-time graph:
```rust
// Attempt to generate compile-time graph
pub struct CompiledGraph {
    gain: Gain,
    filter: Filter,
    osc: Oscillator,
}

impl CompiledGraph {
    fn process(&mut self) -> f32 {
        // ❌ How do we connect them?
        // All we can do is:
        let osc_out = self.osc.process(sr, &mut context);  // f32
        let filter_out = self.filter.process(sr, &mut context);  // f32
        // But how does filter get osc's output?
        // Need to pass it through context... back to runtime!
    }
}
```

The compiler can't generate the connections because it doesn't know the **structure** of each node's I/O.

### Problem 2: Indexed Access - No Named Fields

```rust
// Inputs accessed by index
context.stream(0)  // What is input 0?
context.stream(1)  // What is input 1?
context.value(2)   // What is input 2?

// The compiler doesn't know what these mean!
// It needs runtime metadata to map:
// - "stream(0)" -> "the 'input' field"
// - "value(1)" -> "the 'cutoff' field"
```

**Why this blocks compile-time generation:**

```rust
// Attempt to generate field access
impl CompiledGraph {
    fn process(&mut self) -> f32 {
        // We want to generate:
        self.filter.io.input = self.osc.io.output;

        // But we can't! Because all we have is:
        let osc_out = self.osc.process(sr, &mut context);  // Returns f32

        // To set filter.input, we'd need to put it in context at index 0
        // Which means... we need a runtime context again!
        context.scalar_inputs[0] = osc_out;  // Runtime indirection
        self.filter.process(sr, &mut context);
    }
}
```

### Problem 3: Single Return Value - Can't Express Multiple Outputs

```rust
fn process(...) -> f32 {  // Only one output!
    io.output_left = process_left();
    io.output_right = process_right();

    return io.output_left;  // ❌ output_right is lost
}
```

**For compile-time generation**, we need to route **all** outputs:
```rust
// Want to generate:
self.reverb_io.input_left = self.stereo_io.output_left;
self.reverb_io.input_right = self.stereo_io.output_right;

// But with single return value:
let output = self.stereo.process(...);  // Only get one value!
// Can't route both channels
```

### Problem 4: Runtime Connection Metadata

```rust
// Graph stores connections as runtime data
struct Graph {
    connections: SecondaryMap<ValueKey, Vec<ValueKey>>,  // Runtime lookup!
    //                         ↑              ↑
    //                    output key    input keys
}

// To find what connects to what:
for &target_input in connections.get(output_key) {  // Runtime iteration
    endpoints.get_mut(target_input).set_scalar(output);  // Runtime lookup
}
```

**At compile time**, we need **static** connections, not runtime data structures.

---

## New Pattern: How IO Structs Enable Compile-Time Generation

### The New API (Internal)

```rust
trait SignalProcessor {
    // Each node has its own IO type (even if internal for now)
    // Gain has GainIO, Filter has FilterIO, etc.
}

// Generated IO structs:
pub struct GainIO {
    pub input: f32,    // Named field!
    pub output: f32,   // Named field!
}

pub struct FilterIO {
    pub input: f32,
    pub cutoff_mod: f32,
    pub output: f32,
}

pub struct OscillatorIO {
    pub frequency_mod: f32,
    pub output: f32,
}
```

### Solution 1: Concrete Types - Compiler Knows I/O Structure

```rust
// Each node has a DIFFERENT IO type
let gain_io = GainIO { input: 0.0, output: 0.0 };
let filter_io = FilterIO { input: 0.0, cutoff_mod: 0.0, output: 0.0 };
let osc_io = OscillatorIO { frequency_mod: 0.0, output: 0.0 };

// Compiler knows:
// - Gain has: input, output
// - Filter has: input, cutoff_mod, output
// - Osc has: frequency_mod, output

// At compile time, can generate specific structs!
```

**Enables compile-time generation:**

```rust
compile_time_graph! {
    nodes {
        osc = Oscillator::sine(440.0);
        filter = Filter::new(1000.0);
        gain = Gain::new(0.8);
    }

    connections {
        osc.output -> filter.input;
        filter.output -> gain.input;
    }
}

// Macro generates:
pub struct MySynth {
    osc: Oscillator,
    filter: Filter,
    gain: Gain,

    // Persistent IO structs (concrete types!)
    osc_io: OscillatorIO,
    filter_io: FilterIO,
    gain_io: GainIO,
}

impl MySynth {
    fn process(&mut self) -> f32 {
        // ✅ Compiler can generate direct field assignments!
        self.filter_io.input = self.osc_io.output;  // Known types and fields
        self.gain_io.input = self.filter_io.output;

        // ✅ Direct calls (no dynamic dispatch)
        self.osc.process(44100.0, &mut self.osc_io);
        self.filter.process(44100.0, &mut self.filter_io);
        self.gain.process(44100.0, &mut self.gain_io);

        self.gain_io.output
    }
}
```

The compiler now knows:
- `osc_io` is `OscillatorIO` with an `output` field
- `filter_io` is `FilterIO` with `input` and `output` fields
- Can generate the exact struct field access code

### Solution 2: Named Fields - Declarative Connections

```rust
// Instead of indexed access:
context.stream(0)  // ❌ What is this?

// We have named fields:
io.input           // ✅ Clear what this is
io.cutoff_mod      // ✅ Clear what this is
io.output          // ✅ Clear what this is
```

**Enables compile-time connection generation:**

```rust
// DSL with named connections:
connections {
    osc.output -> filter.input;       // Named fields!
    osc.output -> filter.cutoff_mod;  // Named fields!
    filter.output -> gain.input;      // Named fields!
}

// Macro can generate compile-time field assignments:
self.filter_io.input = self.osc_io.output;
self.filter_io.cutoff_mod = self.osc_io.output;
self.gain_io.input = self.filter_io.output;

// The compiler verifies:
// ✅ osc_io.output exists
// ✅ filter_io.input exists
// ✅ Types match (both f32)
```

### Solution 3: Multiple Outputs - Multiple Struct Fields

```rust
pub struct StereoProcessorIO {
    pub input: f32,
    pub output_left: f32,   // Output 1
    pub output_right: f32,  // Output 2
}

// In DSL:
connections {
    stereo.output_left -> reverb.input_left;
    stereo.output_right -> reverb.input_right;
}

// Generated code:
self.reverb_io.input_left = self.stereo_io.output_left;
self.reverb_io.input_right = self.stereo_io.output_right;
```

All outputs are accessible as struct fields!

### Solution 4: Static Connections - No Runtime Lookups

```rust
// Old: Runtime data structure
connections: HashMap<ValueKey, Vec<ValueKey>>  // ❌ Runtime lookup

// New: Compile-time code generation
self.filter_io.input = self.osc_io.output;     // ✅ Direct assignment
```

No loops, no lookups, just direct assignments generated at compile time.

---

## Complete Example: Runtime vs Compile-Time

### Runtime Graph (Old Pattern)

```rust
// Runtime graph construction
let mut graph = Graph::new(44100.0);
let osc = graph.add_node(Oscillator::sine(440.0));
let filter = graph.add_node(Filter::new(1000.0));
let gain = graph.add_node(Gain::new(0.8));

graph.connect(osc.output, filter.input);
graph.connect(filter.output, gain.input);

// Process (with all the overhead)
loop {
    graph.process();  // Dynamic dispatch, SlotMap lookups, etc.
}
```

**What happens at runtime:**
```rust
for node_key in node_order {  // Iterate nodes
    let node = nodes.get_mut(node_key);  // SlotMap lookup

    // Gather inputs from endpoints
    for &input_key in node.inputs {  // Iterate inputs
        let value = endpoints.get(input_key);  // SlotMap lookup
        input_array[idx] = value;
    }

    // Create context
    let context = ProcessingContext::new(input_array, ...);

    // Call node (dynamic dispatch)
    let output = node.processor.process(sr, &mut context);  // Box<dyn>

    // Route output to connections
    for &target_key in connections.get(output_key) {  // Iterate connections
        endpoints.get_mut(target_key).set_scalar(output);  // SlotMap lookup
    }
}
```

**Overhead per sample:**
- ~10-20 SlotMap lookups
- Dynamic dispatch (virtual call)
- Heap allocations for context
- Connection iteration

### Compile-Time Graph (New Pattern Enabled)

```rust
compile_time_graph! {
    name: MySynth;

    nodes {
        osc = Oscillator::sine(440.0);
        filter = Filter::new(1000.0);
        gain = Gain::new(0.8);
    }

    connections {
        osc.output -> filter.input;
        filter.output -> gain.input;
    }
}

// Process
let mut synth = MySynth::new();
loop {
    let sample = synth.process();  // All optimized!
}
```

**Generated code:**
```rust
pub struct MySynth {
    // Nodes as struct fields (not Box<dyn>!)
    osc: Oscillator,
    filter: Filter,
    gain: Gain,

    // Persistent IO structs
    osc_io: OscillatorIO,
    filter_io: FilterIO,
    gain_io: GainIO,
}

impl MySynth {
    #[inline]  // Compiler can inline!
    pub fn process(&mut self) -> f32 {
        // Direct struct field copies (no SlotMap!)
        self.filter_io.input = self.osc_io.output;
        self.gain_io.input = self.filter_io.output;

        // Direct calls (no dynamic dispatch!)
        self.osc.process_internal(&mut self.osc_io);
        self.filter.process_internal(&mut self.filter_io);
        self.gain.process_internal(&mut self.gain_io);

        self.gain_io.output
    }
}
```

**Overhead per sample:**
- Zero SlotMap lookups (direct field access)
- Zero dynamic dispatch (direct calls)
- Zero heap allocations (stack only)
- Zero connection iteration (direct assignments)

**LLVM can optimize this to:**
```asm
; Entire graph might compile to ~20 CPU instructions:
movss  xmm0, [osc_io.output]      ; Load osc output
movss  [filter_io.input], xmm0    ; Store to filter input
call   filter_process              ; (gets inlined)
movss  xmm0, [filter_io.output]
movss  [gain_io.input], xmm0
mulss  xmm0, [gain_value]          ; Inline gain multiply
movss  [gain_io.output], xmm0
ret
```

The entire graph becomes a few dozen CPU instructions!

---

## Why IO Structs Are The Key

The IO struct pattern provides the **compile-time type information** needed for generation:

| Requirement | Old Pattern | New Pattern (IO Structs) |
|-------------|-------------|--------------------------|
| **Know I/O structure** | ❌ All use `ProcessingContext` | ✅ Each node has `NodeIO` type |
| **Named fields** | ❌ Indexed: `context.stream(0)` | ✅ Named: `io.input` |
| **Multiple outputs** | ❌ Single `f32` return | ✅ Multiple fields: `io.out1`, `io.out2` |
| **Concrete types** | ❌ Type erasure via trait object | ✅ Concrete struct types |
| **Static connections** | ❌ Runtime `HashMap` | ✅ Compile-time field assignments |
| **Inlining** | ❌ Virtual calls can't inline | ✅ Direct calls can inline |

Without IO structs:
- Compiler sees: `fn process() -> f32` (all the same)
- Can't generate specific connections
- Falls back to runtime dispatch

With IO structs:
- Compiler sees: `GainIO`, `FilterIO`, `OscillatorIO` (all different)
- Can generate: `filter_io.input = osc_io.output`
- No runtime dispatch needed

---

## Performance Impact

Based on similar systems (CMajor, Gen~, FAUST):

### Runtime Graph (Current)
```
1.0x baseline
- Dynamic dispatch
- SlotMap lookups
- Runtime connections
```

### Compile-Time Graph (Enabled by IO Structs)
```
15-20x faster
- Static dispatch
- Direct field access
- Compile-time connections
- Full inlining
```

The IO struct pattern is the **foundation** that makes compile-time generation possible!

---

## Summary

**The core insight:**

Old pattern hides I/O structure behind dynamic types → compiler can't see it → must use runtime

New pattern exposes I/O structure as concrete types → compiler can see it → can generate static code

The IO struct is like a **window into the node's I/O** that the compiler can see and reason about at compile time. Without it, everything is opaque and requires runtime metadata.
