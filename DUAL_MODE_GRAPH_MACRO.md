# Dual-Mode Graph Macro: Compile-Time vs Runtime

## The Goal

Have a **single graph definition** that can be compiled in either mode:

```rust
graph! {
    name: MySynth;

    // 👇 Choose compilation mode
    mode: CompileTime;  // or Runtime

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
```

Same graph definition, different generated code!

---

## Design Options

### Option 1: Mode Parameter in Macro (Recommended)

```rust
graph! {
    name: MySynth;
    mode: CompileTime;  // or Runtime (default)

    // ... rest of graph definition
}
```

**Pros:**
- Single DSL, choose mode per-graph
- Clear and explicit
- Can have different modes in same crate

**Cons:**
- Slightly more verbose

### Option 2: Cargo Feature Flag

```toml
# Cargo.toml
[features]
compile-time-graphs = []
```

```rust
// Always the same syntax
graph! {
    name: MySynth;
    // Mode chosen at build time via feature flag
}
```

**Pros:**
- No syntax changes
- One build mode for entire crate

**Cons:**
- All-or-nothing (all graphs compile-time or all runtime)
- Can't mix modes in same binary

### Option 3: Separate Macros with Shared Syntax

```rust
// Runtime graph
graph! { /* ... */ }

// Compile-time graph
compile_time_graph! { /* same syntax */ }
```

**Pros:**
- Clear which is which
- Can convert by changing macro name
- Different macros can have mode-specific features

**Cons:**
- Two macros to maintain
- Not as DRY

### Option 4: Attribute on Graph

```rust
#[compile_time]
graph! {
    name: MySynth;
    // ...
}

// vs

#[runtime]  // or no attribute for default
graph! {
    name: MySynth;
    // ...
}
```

**Pros:**
- Clean syntax
- Rust-idiomatic (like `#[derive(...)]`)

**Cons:**
- Attributes on macro calls are less common
- Might be confusing

---

## Recommended Implementation: Mode Parameter

I recommend **Option 1** with a mode parameter:

```rust
graph! {
    name: VoiceSynth;
    mode: CompileTime;  // 👈 Explicit choice

    input value frequency = 440.0;
    input event gate;
    output stream audio_out;

    nodes {
        osc = PolyBlepOscillator::saw(frequency, 0.6);
        env = AdsrEnvelope::new(0.01, 0.1, 0.7, 0.3);
        filter = TptFilter::new(1000.0, 0.707);
        gain = Gain::new(0.8);
    }

    connections {
        gate -> env.gate;
        osc.output -> filter.input;
        filter.output -> gain.input;
    }

    outputs {
        audio_out = gain.output;
    }
}
```

---

## What Gets Generated: Side-by-Side

### Runtime Mode (Current Behavior)

```rust
graph! {
    name: MySynth;
    mode: Runtime;  // or omit for default
    // ...
}

// Generates:
pub struct MySynth {
    graph: Graph,  // 👈 Contains runtime graph

    // Endpoint handles for external access
    pub osc: OscillatorEndpoints,
    pub filter: FilterEndpoints,
    pub gain: GainEndpoints,
    pub audio_out: StreamOutput,
}

impl MySynth {
    pub fn new(sample_rate: f32) -> Self {
        let mut graph = Graph::new(sample_rate);

        // Add nodes to runtime graph
        let osc = graph.add_node(Oscillator::sine(440.0));
        let filter = graph.add_node(Filter::new(1000.0));
        // ...

        // Create connections
        graph.connect(osc.output, filter.input);
        // ...

        Self { graph, osc, filter, /* ... */ }
    }

    pub fn process(&mut self) -> Result<(), GraphError> {
        self.graph.process()  // 👈 Runtime processing
    }

    pub fn get_output(&self) -> f32 {
        self.graph.get_output(self.audio_out)
    }
}
```

**Characteristics:**
- Uses runtime `Graph`
- Dynamic dispatch (`Box<dyn SignalProcessor>`)
- SlotMap lookups
- Flexible (can modify if you expose the graph)

### CompileTime Mode (New Behavior)

```rust
graph! {
    name: MySynth;
    mode: CompileTime;  // 👈 Compile-time optimization
    // ...
}

// Generates:
pub struct MySynth {
    // Nodes as direct fields (not Box<dyn>!)
    osc: Oscillator,
    filter: Filter,
    gain: Gain,

    // Persistent IO structs
    osc_io: OscillatorIO,
    filter_io: FilterIO,
    gain_io: GainIO,

    // Parameter storage
    frequency: f32,
}

impl MySynth {
    pub fn new(sample_rate: f32) -> Self {
        let mut synth = Self {
            osc: Oscillator::sine(440.0),
            filter: Filter::new(1000.0),
            gain: Gain::new(0.8),
            osc_io: OscillatorIO { frequency_mod: 0.0, output: 0.0 },
            filter_io: FilterIO { input: 0.0, f_mod: 0.0, output: 0.0 },
            gain_io: GainIO { input: 0.0, output: 0.0 },
            frequency: 440.0,
        };

        // Initialize nodes
        synth.osc.init(sample_rate);
        synth.filter.init(sample_rate);
        synth.gain.init(sample_rate);

        synth
    }

    #[inline]  // 👈 Can fully inline!
    pub fn process(&mut self) -> f32 {
        // Direct struct field assignments (compile-time connections)
        self.filter_io.input = self.osc_io.output;
        self.gain_io.input = self.filter_io.output;

        // Direct calls (no virtual dispatch)
        self.osc.process_with_io(&mut self.osc_io);
        self.filter.process_with_io(&mut self.filter_io);
        self.gain.process_with_io(&mut self.gain_io);

        self.gain_io.output
    }

    // Parameter setters
    pub fn set_frequency(&mut self, freq: f32) {
        self.frequency = freq;
        self.osc.frequency = freq;
    }

    pub fn get_output(&self) -> f32 {
        self.gain_io.output
    }
}
```

**Characteristics:**
- No runtime `Graph`
- Direct calls (fully inlined)
- Direct field access (no lookups)
- Fixed structure, fast execution

---

## API Compatibility

The **external API stays mostly the same**:

```rust
// Works with both modes!
let mut synth = MySynth::new(44100.0);

// Set parameters
synth.set_frequency(880.0);

// Process audio
loop {
    let sample = synth.process();  // CompileTime: returns f32
                                   // Runtime: returns Result
    output_buffer.push(sample);
}
```

**Key differences:**

| Feature | Runtime Mode | CompileTime Mode |
|---------|--------------|------------------|
| `new()` | ✅ Same | ✅ Same |
| `process()` | Returns `Result<(), GraphError>` | Returns `f32` |
| `set_parameters()` | ✅ Same | ✅ Same |
| Modify structure | ✅ Can expose `graph` field | ❌ Fixed at compile time |
| Performance | 1x baseline | 15-20x faster |

---

## Implementation Strategy

### Phase 1: Extend Existing `graph!` Macro

Modify `oscen-macros/src/graph_macro/codegen.rs`:

```rust
pub enum CompileMode {
    Runtime,      // Default, current behavior
    CompileTime,  // New optimized mode
}

pub struct GraphSpec {
    pub name: Ident,
    pub mode: CompileMode,  // 👈 New field
    pub inputs: Vec<InputDecl>,
    pub outputs: Vec<OutputDecl>,
    pub nodes: Vec<NodeDecl>,
    pub connections: Vec<ConnectionDecl>,
}

pub fn generate_graph_struct(spec: &GraphSpec) -> TokenStream {
    match spec.mode {
        CompileMode::Runtime => generate_runtime_graph(spec),
        CompileMode::CompileTime => generate_compile_time_graph(spec),
    }
}
```

### Phase 2: Parse Mode Parameter

Modify `oscen-macros/src/graph_macro/parse.rs`:

```rust
fn parse_graph_spec(input: ParseStream) -> Result<GraphSpec> {
    // Parse: name: MySynth;
    let name = parse_name(input)?;

    // Parse optional: mode: CompileTime;
    let mode = if input.peek(kw::mode) {
        input.parse::<kw::mode>()?;
        input.parse::<Token![:]>()?;
        let mode_ident: Ident = input.parse()?;
        match mode_ident.to_string().as_str() {
            "Runtime" => CompileMode::Runtime,
            "CompileTime" => CompileMode::CompileTime,
            _ => return Err(Error::new(mode_ident.span(), "expected Runtime or CompileTime")),
        }
    } else {
        CompileMode::Runtime  // Default
    };

    // ... rest of parsing
}
```

### Phase 3: Generate Compile-Time Code

New function in `codegen.rs`:

```rust
fn generate_compile_time_graph(spec: &GraphSpec) -> TokenStream {
    let name = &spec.name;

    // Generate node fields
    let node_fields = spec.nodes.iter().map(|node| {
        let node_name = &node.name;
        let node_type = &node.ty;
        quote! { #node_name: #node_type }
    });

    // Generate IO fields
    let io_fields = spec.nodes.iter().map(|node| {
        let node_name = &node.name;
        let io_type = format_ident!("{}IO", node.ty);
        let io_field = format_ident!("{}_io", node_name);
        quote! { #io_field: #io_type }
    });

    // Generate connection assignments
    let connections = spec.connections.iter().map(|conn| {
        let from_node = &conn.from.node;
        let from_output = &conn.from.field;
        let to_node = &conn.to.node;
        let to_input = &conn.to.field;

        quote! {
            self.#to_node_io.#to_input = self.#from_node_io.#from_output;
        }
    });

    // Generate process calls
    let process_calls = spec.nodes.iter().map(|node| {
        let node_name = &node.name;
        let io_field = format_ident!("{}_io", node_name);
        quote! {
            self.#node_name.process_with_io(&mut self.#io_field);
        }
    });

    quote! {
        pub struct #name {
            #(#node_fields,)*
            #(#io_fields,)*
        }

        impl #name {
            pub fn new(sample_rate: f32) -> Self {
                // ... initialization
            }

            #[inline]
            pub fn process(&mut self) -> f32 {
                // Connection assignments
                #(#connections)*

                // Process calls
                #(#process_calls)*

                // Return final output
                self.gain_io.output
            }
        }
    }
}
```

---

## Usage Examples

### Example 1: Voice (Compile-Time for Speed)

```rust
graph! {
    name: SynthVoice;
    mode: CompileTime;  // 👈 Fast voice processing

    input value frequency = 440.0;
    input event gate;
    output stream audio_out;

    nodes {
        osc = Oscillator::sine(frequency);
        env = AdsrEnvelope::new(0.01, 0.1, 0.7, 0.3);
        filter = Filter::new(1000.0);
        gain = Gain::new(0.8);
    }

    connections {
        gate -> env.gate;
        osc.output -> filter.input;
        filter.output -> gain.input;
    }

    outputs {
        audio_out = gain.output;
    }
}

// 16 fully-optimized voices
let voices: [SynthVoice; 16] = array::from_fn(|_| SynthVoice::new(44100.0));
```

### Example 2: Effect Chain (Runtime for Flexibility)

```rust
graph! {
    name: EffectChain;
    mode: Runtime;  // 👈 User can modify effects

    input stream audio_in;
    output stream audio_out;

    nodes {
        chorus = Chorus::new();
        delay = Delay::new(0.5, 0.3);
        reverb = Reverb::new();
    }

    connections {
        audio_in -> chorus.input;
        chorus.output -> delay.input;
        delay.output -> reverb.input;
    }

    outputs {
        audio_out = reverb.output;
    }
}

// Can modify the effect chain at runtime
let mut effects = EffectChain::new(44100.0);
effects.graph.add_node(Distortion::new());  // Runtime modification
```

### Example 3: Hybrid System

```rust
struct Synthesizer {
    // Compile-time: 16 optimized voices
    voices: [SynthVoice; 16],

    // Runtime: flexible master effects
    master_effects: EffectChain,
}

impl Synthesizer {
    fn process(&mut self) -> f32 {
        // Sum all voices (fully optimized)
        let voice_sum: f32 = self.voices.iter_mut()
            .map(|v| v.process())
            .sum();

        // Process through effects (flexible)
        self.master_effects.process_sample(voice_sum)
    }
}
```

---

## Migration Path

### Step 1: Keep Current Behavior by Default

```rust
// No changes needed - defaults to runtime mode
graph! {
    name: MySynth;
    // mode: Runtime is implicit
}
```

### Step 2: Opt-In to Compile-Time

```rust
// Add one line to enable optimization
graph! {
    name: MySynth;
    mode: CompileTime;  // 👈 Just add this
    // ... rest stays the same
}
```

### Step 3: Benchmark and Iterate

```rust
#[cfg(feature = "optimize-voices")]
const VOICE_MODE: &str = "CompileTime";
#[cfg(not(feature = "optimize-voices"))]
const VOICE_MODE: &str = "Runtime";

// Could even use a macro to inject the mode
graph! {
    name: SynthVoice;
    mode: #VOICE_MODE;  // Set via feature flag
}
```

---

## Benefits

✅ **Same DSL** - learn once, use in both modes
✅ **Easy migration** - add one line to optimize
✅ **Flexible** - choose mode per-graph
✅ **Gradual adoption** - optimize hot paths first
✅ **Safe** - compiler enforces mode constraints

## The Answer

**Yes, it would be very easy!** Just add:

```rust
mode: CompileTime;  // or Runtime
```

to any graph definition. The macro can generate completely different code based on this flag while keeping the same graph specification.

---

## Implementation Effort

**Estimated work**: 1-2 weeks

1. **Parser changes** (1 day): Add mode parameter parsing
2. **Code generation** (3-5 days): Implement compile-time generation
3. **Testing** (2-3 days): Ensure both modes work correctly
4. **Documentation** (1-2 days): Examples and migration guide

**Difficulty**: Moderate - most of the hard work (IO structs) is already done!
