# Compile-Time Graphs: Flexibility vs Performance Trade-offs

## The Fundamental Trade-off

**Compile-time graph optimization requires static structure**

The entire reason compile-time graphs are 15-20x faster is because the compiler can:
1. Inline everything
2. Eliminate dead code
3. Optimize across nodes
4. Generate direct field copies

All of these require **knowing the graph structure at compile time**.

---

## What CAN Be Modified at Runtime

### ✅ Parameter Values

Even with compile-time graphs, you can still change **parameter values**:

```rust
compile_time_graph! {
    name: Synth;

    nodes {
        osc = Oscillator::sine(440.0);    // Initial frequency
        filter = Filter::new(1000.0);     // Initial cutoff
        gain = Gain::new(0.8);            // Initial gain
    }

    connections {
        osc.output -> filter.input;
        filter.output -> gain.input;
    }
}

// At runtime, you can change parameters:
let mut synth = Synth::new();

// ✅ Modify parameters (fast)
synth.set_frequency(880.0);     // Change oscillator frequency
synth.set_cutoff(2000.0);       // Change filter cutoff
synth.set_gain(0.5);            // Change gain amount

// Graph structure remains fixed, but sounds different!
loop {
    let sample = synth.process();  // Still fully optimized
}
```

**How this works:**

```rust
pub struct Synth {
    osc: Oscillator,        // Holds frequency parameter
    filter: Filter,         // Holds cutoff parameter
    gain: Gain,             // Holds gain parameter

    osc_io: OscillatorIO,
    filter_io: FilterIO,
    gain_io: GainIO,
}

impl Synth {
    pub fn set_frequency(&mut self, freq: f32) {
        self.osc.frequency = freq;  // ✅ Just updating a field
    }

    pub fn set_cutoff(&mut self, cutoff: f32) {
        self.filter.cutoff = cutoff;  // ✅ Just updating a field
    }

    #[inline]
    pub fn process(&mut self) -> f32 {
        // Graph structure is fixed, but parameters can vary
        self.filter_io.input = self.osc_io.output;
        self.gain_io.input = self.filter_io.output;

        self.osc.process(&mut self.osc_io);      // Uses current frequency
        self.filter.process(&mut self.filter_io); // Uses current cutoff
        self.gain.process(&mut self.gain_io);    // Uses current gain

        self.gain_io.output
    }
}
```

**Performance**: No overhead! Changing parameters is just setting struct fields.

---

## What CANNOT Be Modified at Runtime

### ❌ Graph Structure

You **cannot** change the structure of a compile-time graph:

```rust
// ❌ Cannot do this with compile-time graph:
synth.add_node(Reverb::new());              // Can't add nodes
synth.remove_node(synth.filter);            // Can't remove nodes
synth.disconnect(osc.output, filter.input); // Can't change connections
synth.connect(osc.output, reverb.input);    // Can't add connections
```

**Why not?**

Because the graph structure is compiled into the code:

```rust
// This is the generated process() code:
pub fn process(&mut self) -> f32 {
    self.filter_io.input = self.osc_io.output;  // ← Hard-coded connection
    self.gain_io.input = self.filter_io.output; // ← Hard-coded connection

    self.osc.process(&mut self.osc_io);         // ← Hard-coded calls
    self.filter.process(&mut self.filter_io);
    self.gain.process(&mut self.gain_io);

    self.gain_io.output
}
```

The connections and call order are **in the machine code**. You'd need to recompile to change them.

---

## Hybrid Approaches: Best of Both Worlds

### Option 1: Switchable Sub-Graphs (Const Generics)

Use const generics for compile-time variants:

```rust
compile_time_graph! {
    name: Synth<const FILTER_TYPE: usize>;  // Compile-time parameter

    nodes {
        osc = Oscillator::sine(440.0);

        // Different filter types compiled separately
        #[switch(FILTER_TYPE)]
        filter = match FILTER_TYPE {
            0 => IirLowpass::new(1000.0),
            1 => TptFilter::new(1000.0),
            2 => StateVariableFilter::new(1000.0),
        };

        gain = Gain::new(0.8);
    }

    connections {
        osc.output -> filter.input;
        filter.output -> gain.input;
    }
}

// At runtime, choose which variant:
let synth_iir = Synth::<0>::new();      // IIR filter (separate compiled code)
let synth_tpt = Synth::<1>::new();      // TPT filter (separate compiled code)
let synth_svf = Synth::<2>::new();      // SVF filter (separate compiled code)

// Each variant is fully optimized for its configuration
// Can't switch between them without creating a new instance
```

### Option 2: Optional Nodes with Runtime Enabling

Use `Option` for nodes that can be bypassed:

```rust
compile_time_graph! {
    name: Synth;

    nodes {
        osc = Oscillator::sine(440.0);

        #[optional]
        distortion = Distortion::new(0.5);  // Can be enabled/disabled

        filter = Filter::new(1000.0);
        gain = Gain::new(0.8);
    }

    connections {
        osc.output -> distortion.input;
        distortion.output -> filter.input;  // Or bypass to filter
        filter.output -> gain.input;
    }
}

// Generated code:
pub struct Synth {
    osc: Oscillator,
    distortion: Option<Distortion>,  // ← Optional node
    filter: Filter,
    gain: Gain,
}

impl Synth {
    pub fn enable_distortion(&mut self, enabled: bool) {
        if enabled && self.distortion.is_none() {
            self.distortion = Some(Distortion::new(0.5));
        } else if !enabled {
            self.distortion = None;
        }
    }

    pub fn process(&mut self) -> f32 {
        self.osc.process(&mut self.osc_io);

        // Runtime branch (slight overhead, but optimizable)
        let osc_out = self.osc_io.output;
        let filtered_in = if let Some(dist) = &mut self.distortion {
            self.distortion_io.input = osc_out;
            dist.process(&mut self.distortion_io);
            self.distortion_io.output
        } else {
            osc_out  // Bypass distortion
        };

        self.filter_io.input = filtered_in;
        self.filter.process(&mut self.filter_io);
        self.gain_io.input = self.filter_io.output;
        self.gain.process(&mut self.gain_io);

        self.gain_io.output
    }
}
```

**Cost**: Small branch prediction overhead, but compiler can optimize well.

### Option 3: Dual-Mode System

Use **both** compile-time and runtime graphs in the same application:

```rust
// Performance-critical parts: Compile-time graphs
struct VoiceProcessor {
    voice_graph: CompiledVoiceGraph,  // ← Compile-time (fast!)
}

// Flexible parts: Runtime graphs
struct EffectChain {
    effects: Graph,  // ← Runtime (flexible!)
}

struct Synthesizer {
    // Fixed voice processing (needs speed)
    voices: [VoiceProcessor; 16],

    // Flexible effect chain (changed less frequently)
    master_effects: EffectChain,
}

impl Synthesizer {
    fn process(&mut self) -> f32 {
        // Voices use compile-time graphs (fast)
        let voice_sum: f32 = self.voices.iter_mut()
            .map(|v| v.voice_graph.process())  // Fully optimized
            .sum();

        // Master effects use runtime graph (flexible)
        self.master_effects.process_sample(voice_sum)  // Can modify effects
    }
}
```

**Benefits**:
- Hot path (voices) is fully optimized
- Effect chain can be modified by user
- Best of both worlds!

### Option 4: Hot-Reload with Recompilation

Use dynamic library reloading for graph changes:

```rust
// Main application
struct App {
    synth_lib: DynamicLibrary,  // Loaded .so/.dll
}

impl App {
    fn reload_graph(&mut self, new_graph_dsl: &str) {
        // 1. Generate new graph code from DSL
        let code = compile_graph(new_graph_dsl);

        // 2. Compile to dynamic library
        compile_to_dylib(code, "synth_v2.so");

        // 3. Hot-reload the library
        self.synth_lib = DynamicLibrary::open("synth_v2.so");

        // 4. Continue processing with new graph (optimized!)
    }
}
```

**This is what CMajor does!** Graph changes require recompilation, but:
- Compilation is fast (<100ms)
- New code is fully optimized
- Can do this while audio is running (hot reload)

---

## Comparison Table

| Approach | Graph Changes | Performance | Use Case |
|----------|---------------|-------------|----------|
| **Pure compile-time** | ❌ None | ⚡⚡⚡ Fastest | Fixed instruments, plugins |
| **Runtime graph** | ✅ Full flexibility | 🐌 Slowest | Modular synths, live coding |
| **Const generic variants** | ⚡ Compile-time only | ⚡⚡⚡ Fastest | Fixed set of modes |
| **Optional nodes** | ⚡ Enable/disable | ⚡⚡ Fast | Configurable effects |
| **Dual-mode** | ✅ Some parts flexible | ⚡⚡⚡/🐌 Mixed | Hybrid systems |
| **Hot-reload** | ✅ Any (with recompile) | ⚡⚡⚡ Fastest | Development, DAWs |

---

## Recommended Strategy for Oscen

### Phase 1: Pure Compile-Time Graphs
Start with non-modifiable graphs for maximum performance:

```rust
compile_time_graph! {
    name: MonoSynth;
    // Fixed structure, variable parameters
}
```

**Good for**:
- VST/AU plugins (fixed DSP graph)
- Standalone instruments
- Performance-critical inner loops

### Phase 2: Add Parameter Modulation
Make parameters runtime-accessible:

```rust
synth.set_cutoff(cutoff);
synth.set_resonance(resonance);
```

**Good for**:
- MIDI CC control
- Automation
- LFO modulation

### Phase 3: Optional Nodes
Add ability to enable/disable effects:

```rust
synth.enable_chorus(true);
synth.enable_reverb(false);
```

**Good for**:
- User-configurable effects
- Preset variations

### Phase 4: Dual-Mode System
Combine compile-time and runtime graphs:

```rust
struct Synth {
    voice: CompiledVoiceGraph,   // Fast voice processing
    effects: RuntimeGraph,        // Flexible effects
}
```

**Good for**:
- Complex applications
- DAWs
- Modular systems

### Phase 5: Hot-Reload (Optional)
Add hot-reload for development:

```rust
app.reload_graph_from_file("my_synth.graph");
```

**Good for**:
- Live coding
- Rapid iteration
- Plugin development

---

## The Bottom Line

**Compile-time graphs are not modifiable at runtime** - that's the whole point! The graph structure being **static** is what enables the massive performance gains.

**But you have options**:

1. ✅ **Parameters** can always be changed (no performance cost)
2. ✅ **Const generics** give compile-time variants
3. ✅ **Optional nodes** give runtime enable/disable
4. ✅ **Dual-mode** combines fast + flexible
5. ✅ **Hot-reload** allows "runtime" changes via recompilation

Most real-world systems use a **hybrid approach**: Compile-time graphs for the performance-critical parts, runtime graphs for the flexible parts.

---

## Example: Real-World Use Cases

### VST Plugin (Pure Compile-Time)
```rust
compile_time_graph! {
    name: MyCompressor;
    // Graph structure is fixed
    // Parameters exposed to DAW
}
```
- Graph never changes
- Parameters automated by DAW
- Maximum performance

### Modular Synth (Runtime Graphs)
```rust
let mut graph = Graph::new();
graph.add_node(...);
graph.connect(...);
// User patches cables
```
- Full flexibility
- Accepts slower performance
- User expects patching

### Hybrid Synth (Best of Both)
```rust
struct HybridSynth {
    voices: [CompiledVoice; 16],  // Fast
    effects: Graph,                // Flexible
}
```
- Voices fully optimized
- Effects user-configurable
- Best of both worlds

The key insight: **Choose the right tool for each part of your system!**
