# Future Optimizations Enabled by Struct-of-Arrays I/O

This document explains how the struct-of-arrays I/O refactoring enables significant future optimizations in Oscen.

## 1. Graph Execution Optimization (Direct Struct Field Copying)

### Current Execution Model

Right now, even though nodes use IO structs internally, the graph execution still goes through the endpoint SlotMap indirection:

```rust
// Current execution in graph_impl.rs (simplified)
for node_key in node_order {
    // 1. Gather inputs from SlotMap
    let input = endpoints.get(input_key).as_scalar();  // Indirection!

    // 2. Create context
    let context = ProcessingContext::new(&[input], ...);

    // 3. Node creates IO struct internally, processes, returns single output
    let output = node.process(sample_rate, &mut context);  // Returns f32

    // 4. Write to output endpoint in SlotMap
    endpoints.get_mut(output_key).set_scalar(output);  // Indirection!

    // 5. Copy to connected inputs
    for &target_input in connections.get(output_key) {
        endpoints.get_mut(target_input).set_scalar(output);  // More indirection!
    }
}
```

**Problems:**
- Multiple SlotMap lookups per sample
- Creates IO struct every sample (even though it's stack-allocated)
- Single output returned (wastes multi-output potential)
- Cache-unfriendly access pattern

### Optimized Execution with Direct Struct Field Copying

With the IO struct pattern, we can eliminate the SlotMap indirection for stream data:

```rust
// Optimized execution (future)
pub struct GraphOptimized {
    // Store persistent IO structs for each node
    node_io: HashMap<NodeKey, Box<dyn Any>>,  // Actually NodeIO structs
    node_processors: HashMap<NodeKey, Box<dyn SignalProcessor>>,

    // Pre-computed copy operations: (src_node, src_field, dst_node, dst_field)
    stream_copies: Vec<CopyOperation>,
}

struct CopyOperation {
    src_node: NodeKey,
    src_field_offset: usize,  // Offset into IO struct
    dst_node: NodeKey,
    dst_field_offset: usize,
}

impl GraphOptimized {
    fn process_sample(&mut self) {
        // 1. Copy between connected nodes BEFORE processing
        for copy_op in &self.stream_copies {
            unsafe {
                // Direct memory copy between struct fields
                let src_io = self.node_io.get(&copy_op.src_node).unwrap();
                let dst_io = self.node_io.get_mut(&copy_op.dst_node).unwrap();

                // Copy f32 from src.output to dst.input at known offsets
                std::ptr::copy_nonoverlapping(
                    (src_io.as_ref() as *const dyn Any as *const u8)
                        .add(copy_op.src_field_offset),
                    (dst_io.as_mut() as *mut dyn Any as *mut u8)
                        .add(copy_op.dst_field_offset),
                    std::mem::size_of::<f32>(),
                );
            }
        }

        // 2. Process each node in topological order
        // Nodes read from their IO struct (already populated) and write to it
        for node_key in &self.node_order {
            let processor = self.node_processors.get_mut(node_key).unwrap();
            let io = self.node_io.get_mut(node_key).unwrap();

            // Process operates on the persistent IO struct
            processor.process_with_io(io);  // No return value needed!
        }

        // Stream data flows via struct field copies, not SlotMap!
    }
}
```

**Benefits:**
- **Zero SlotMap lookups** for stream data during processing
- **Persistent IO structs** - created once, reused every sample
- **Direct memory copies** - CPU can optimize these aggressively
- **Cache-friendly** - sequential struct access, predictable access patterns
- **~2-3x faster** graph execution (based on CMajor's benchmarks)

### Why This Wasn't Possible Before

The old pattern returned a single `f32`, which forced the graph to:
1. Call process()
2. Get the output
3. Manually route it through connections

With IO structs:
1. Outputs are struct fields
2. Graph knows field offsets at connection time
3. Can pre-compute all copy operations
4. Process just reads/writes struct fields

---

## 2. Multiple Output Routing Through All IO Struct Fields

### Current Limitation

Right now, we only return one primary output:

```rust
impl SignalProcessor for MultiOutputNode {
    fn process(&mut self, ...) -> f32 {
        let mut io = MultiOutputNodeIO {
            input: ...,
            output_left: 0.0,
            output_right: 0.0,
        };

        io.output_left = process_left();
        io.output_right = process_right();

        return io.output_left;  // ❌ output_right is lost!
    }
}
```

### Future: Route All IO Struct Fields

With direct struct field access, the graph can route **all** outputs:

```rust
// Define a stereo processor
#[derive(Node)]
pub struct StereoProcessor {
    #[input(stream)]
    input: f32,

    #[output(stream)]
    output_left: f32,   // Output 1

    #[output(stream)]
    output_right: f32,  // Output 2
}

// Generated: StereoProcessorIO { input: f32, output_left: f32, output_right: f32 }

// Graph knows about ALL outputs via reflection on IO struct
graph.connect(stereo.output_left, reverb.input_left);
graph.connect(stereo.output_right, reverb.input_right);

// Execution copies BOTH outputs
for field in node_io_struct.fields_tagged_as_output() {
    for connection in connections.get(node, field) {
        copy_field(src_io, field, dst_io, connection.input_field);
    }
}
```

**Example: Stereo Filter**

```rust
#[derive(Node)]
pub struct StereoFilter {
    #[input(stream)]
    input_left: f32,

    #[input(stream)]
    input_right: f32,

    #[output(stream)]
    output_left: f32,

    #[output(stream)]
    output_right: f32,
}

impl SignalProcessor for StereoFilter {
    fn process(&mut self, ...) -> f32 {
        let mut io = StereoFilterIO {
            input_left: self.get_input_left(context),
            input_right: self.get_input_right(context),
            output_left: 0.0,
            output_right: 0.0,
        };

        // Process both channels
        io.output_left = filter(io.input_left);
        io.output_right = filter(io.input_right);

        // All outputs are in the IO struct!
        // Graph execution can copy both to downstream nodes
        io.output_left  // Primary output for compatibility
    }
}
```

This enables natural **multi-channel audio** (stereo, surround) and **multi-output nodes** (oscillator with multiple waveforms, filter with multiple modes, etc.).

---

## 3. Full Event I/O Support

### Current Event Handling

Events currently go through `ProcessingContext`:

```rust
// Reading events
let events = self.events_gate(context);  // From context
for event in events {
    handle(event);
}

// Emitting events
context.emit_event(output_index, event);  // To context
```

This is indirect - events don't flow through the IO struct yet.

### Future: Events as IO Struct Fields

Make events first-class citizens in the IO struct:

```rust
#[derive(Node)]
pub struct MidiVoiceHandler {
    #[input(event)]
    note_on: (),

    #[input(event)]
    note_off: (),

    #[output(event)]
    gate: (),      // Event output

    #[output(value)]
    frequency: f32,  // Value output
}

// Generated IO struct:
pub struct MidiVoiceHandlerIO<'io> {
    // Event inputs are slices (already have this!)
    pub note_on: &'io [EventInstance],
    pub note_off: &'io [EventInstance],

    // Event outputs are vectors (need to add this!)
    pub gate: Vec<EventInstance>,
}

impl SignalProcessor for MidiVoiceHandler {
    fn process(&mut self, ...) -> f32 {
        let mut io = MidiVoiceHandlerIO {
            note_on: self.events_note_on(context),
            note_off: self.events_note_off(context),
            gate: Vec::new(),  // Output event buffer
        };

        // Read events from IO struct
        for event in io.note_on {
            if let Some(note) = extract_note(event) {
                // Write events to IO struct output
                io.gate.push(EventInstance {
                    frame_offset: event.frame_offset,
                    payload: EventPayload::scalar(1.0),
                });
            }
        }

        // Graph execution copies io.gate to all connected event inputs!
        self.frequency
    }
}
```

**Graph Event Routing:**

```rust
// After processing each node
for event_output_field in node_io.event_output_fields() {
    let events = &node_io[event_output_field];  // Vec<EventInstance>

    for connection in connections.get(node, event_output_field) {
        let target_io = get_node_io(connection.dst_node);
        // Copy events to target's input buffer
        target_io[connection.dst_field].extend(events);
    }
}
```

**Benefits:**
- Events flow through structs like stream data
- No special `emit_event()` API needed
- Graph routing is uniform (streams + events)
- Can inspect event outputs before next node processes

---

## 4. Compile-Time Graph Generation

This is the **biggest** optimization - generate entire graphs as static code.

### Current: Runtime Dynamic Graphs

```rust
// Runtime graph construction
let mut graph = Graph::new(44100.0);
let osc = graph.add_node(Oscillator::sine(440.0, 0.5));
let filter = graph.add_node(TptFilter::new(1000.0, 0.7));
graph.connect(osc.output, filter.input);

// Process uses trait objects
graph.process();  // Dynamic dispatch for each node
```

**Costs:**
- `Box<dyn SignalProcessor>` - virtual calls
- SlotMap lookups for connections
- Runtime topology sort
- No cross-node inlining

### Future: Compile-Time Generated Graphs

Create a macro that generates a **concrete struct** with all nodes as fields:

```rust
// Define graph at compile time
compile_time_graph! {
    name: MySynth;
    sample_rate: 44100.0;

    nodes {
        osc = Oscillator::sine(440.0, 0.5);
        filter = TptFilter::new(1000.0, 0.7);
        gain = Gain::new(0.8);
    }

    connections {
        osc.output -> filter.input;
        filter.output -> gain.input;
    }

    outputs {
        audio_out = gain.output;
    }
}

// Macro generates:
pub struct MySynth {
    // All nodes as struct fields (not Box<dyn>!)
    osc: Oscillator,
    filter: TptFilter,
    gain: Gain,

    // Persistent IO structs
    osc_io: OscillatorIO,
    filter_io: TptFilterIO,
    gain_io: GainIO,
}

impl MySynth {
    // Generated process method - NO dynamic dispatch!
    #[inline]
    pub fn process(&mut self) -> f32 {
        // Direct struct field copies (compiler can inline!)
        self.filter_io.input = self.osc_io.output;
        self.gain_io.input = self.filter_io.output;

        // Direct calls (compiler can inline!)
        self.osc.process(44100.0, &mut self.osc_io);
        self.filter.process(44100.0, &mut self.filter_io);
        self.gain.process(44100.0, &mut self.gain_io);

        // Return final output
        self.gain_io.output
    }
}
```

**Benefits:**
- **Zero dynamic dispatch** - all calls are direct
- **Full inlining** - compiler can inline entire graph
- **No SlotMap** - all data in struct fields
- **LLVM can optimize across nodes** - sees entire graph
- **10-20x faster** than runtime graphs (based on similar systems)

### Why Struct-of-Arrays Makes This Possible

1. **Predictable struct layout** - each node has known IO struct type
2. **Named fields** - can generate `node_a.io.output -> node_b.io.input`
3. **No return value needed** - reads/writes through IO struct
4. **Topology known at compile time** - can generate optimal call order

This is essentially what CMajor does - the entire graph becomes a single optimized function.

---

## 5. SIMD Optimizations

SIMD (Single Instruction, Multiple Data) allows processing multiple samples in parallel.

### Current: Scalar Processing

```rust
// Process one sample at a time
for sample in 0..buffer_size {
    let input = buffer[sample];
    let output = node.process(sample_rate, input);  // One sample
    buffer[sample] = output;
}
```

### Future: Vectorized Processing

Extend IO structs to hold **arrays** of samples:

```rust
const SIMD_WIDTH: usize = 4;  // Process 4 samples at once

// Vectorized IO struct
pub struct GainIO_SIMD {
    pub input: [f32; SIMD_WIDTH],   // 4 samples
    pub output: [f32; SIMD_WIDTH],  // 4 samples
}

impl SignalProcessor for Gain {
    fn process_simd(&mut self, io: &mut GainIO_SIMD, gain: f32) {
        // Use SIMD intrinsics
        use std::arch::x86_64::*;

        unsafe {
            // Load 4 samples at once
            let input_vec = _mm_loadu_ps(io.input.as_ptr());
            let gain_vec = _mm_set1_ps(gain);  // Broadcast gain to 4 lanes

            // Multiply 4 samples in parallel
            let output_vec = _mm_mul_ps(input_vec, gain_vec);

            // Store 4 samples at once
            _mm_storeu_ps(io.output.as_mut_ptr(), output_vec);
        }
    }
}
```

**Graph Execution:**

```rust
// Process 4 samples at a time
for chunk in 0..(buffer_size / SIMD_WIDTH) {
    // Load 4 samples into each node's IO struct
    for node in &nodes {
        for i in 0..SIMD_WIDTH {
            node_io.input[i] = get_connected_output(i);
        }
    }

    // Process 4 samples in parallel through entire graph
    for node in &nodes {
        node.process_simd(&mut node_io);
    }

    // Store 4 output samples
    for i in 0..SIMD_WIDTH {
        output_buffer[chunk * SIMD_WIDTH + i] = final_io.output[i];
    }
}
```

### Why Struct-of-Arrays Enables SIMD

1. **Array layout** - samples are contiguous in memory
   ```rust
   struct GainIO {
       input: [f32; 4],  // ← Perfect for SIMD load
       output: [f32; 4], // ← Perfect for SIMD store
   }
   ```

2. **Predictable data flow** - compiler knows data dependencies
3. **Explicit parallelism** - process multiple samples per call
4. **Cache efficiency** - load 64 bytes (cache line) = 16 f32s at once

### Advanced: Auto-Vectorization

With compile-time graphs + struct-of-arrays, LLVM can **auto-vectorize**:

```rust
// Simple scalar code
for i in 0..N {
    io.output[i] = io.input[i] * gain;
}

// LLVM auto-generates SIMD:
// - Loads 4 samples at once
// - Multiplies 4 samples in parallel
// - Stores 4 samples at once
// All without manual intrinsics!
```

**Performance Impact:**
- **4x throughput** with SSE (4-wide f32 SIMD)
- **8x throughput** with AVX (8-wide f32 SIMD)
- Modern CPUs can do 2-4 SIMD ops per cycle

---

## Performance Comparison

Based on similar systems (CMajor, FAUST, Gen~), here's the expected speedup:

| Optimization | Current | Optimized | Speedup |
|--------------|---------|-----------|---------|
| Base (current runtime graph) | 1.0x | - | - |
| Direct struct field copying | 1.0x | 2.5x | **2.5x faster** |
| Compile-time graph generation | 1.0x | 15x | **15x faster** |
| + SIMD vectorization (4-wide) | 1.0x | 60x | **60x faster** |

These are realistic based on:
- **CMajor**: 10-20x faster than Pure Data/Max
- **FAUST**: 5-15x faster than equivalent C++
- **Compile-time optimization**: Eliminates 90%+ of overhead

---

## Migration Path

1. **Phase 1** (Current): Internal IO structs
   - ✅ Nodes use IO structs internally
   - ✅ Graph execution unchanged
   - ✅ Maintains backward compatibility

2. **Phase 2**: Direct struct field copying
   - Store persistent IO structs in graph
   - Pre-compute copy operations at connection time
   - Eliminate SlotMap for stream data
   - Estimated: **2-3x speedup**

3. **Phase 3**: Full multi-output support
   - Route all IO struct output fields
   - Enable multi-channel and multi-output nodes
   - Event I/O through struct fields

4. **Phase 4**: Compile-time graph generation
   - Macro generates static graph structs
   - Zero dynamic dispatch
   - Full inlining across nodes
   - Estimated: **10-20x speedup**

5. **Phase 5**: SIMD vectorization
   - Array-based IO structs
   - Vectorized node implementations
   - Auto-vectorization in compile-time graphs
   - Estimated: **4-8x additional speedup**

---

## Conclusion

The struct-of-arrays pattern is the **foundation** for all these optimizations:

1. **Named struct fields** enable direct memory copies
2. **Predictable layout** enables compile-time optimization
3. **Array fields** enable SIMD vectorization
4. **No return value** enables multi-output routing
5. **Concrete types** enable compile-time graph generation

Without struct-of-arrays, we'd be stuck with:
- Indexed array access (`context.stream(0)`)
- Single return value
- Dynamic dispatch
- Runtime connection resolution

With struct-of-arrays, we get:
- Named field access (`io.output`)
- Multiple outputs as struct fields
- Potential for static dispatch
- Compile-time connection resolution

This refactoring unlocks Oscen's path to **CMajor-level performance** while maintaining Rust's type safety and ergonomics!
