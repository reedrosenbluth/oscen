# Multi-Channel Stream Support for Oscen

## Motivation

Currently, Oscen streams are always scalar `f32` values. CMajor supports multi-channel streams (e.g., `stream float<32>` for 32 channels), which enables cleaner architectures for:

- **Multi-harmonic synthesis** (e.g., ElectricPiano's 32 amplitude values)
- **Stereo/surround audio** (explicit 2/5.1/7.1 channel routing)
- **Spectral processing** (FFT bins as stream channels)
- **Modulation matrices** (multiple LFOs/envelopes as streams)
- **Sidechain routing** (multi-input compressors, vocoders)

### Example: ElectricPiano (CMajor vs Current Oscen)

**CMajor (clean):**
```cmajor
processor AmplitudeSource {
    output stream float<32> amplitudes;  // 32-channel stream
}

processor OscillatorBank {
    input stream float<32> amplitudes;   // 32-channel stream
    output stream float audioOut;
}

// Connection is simple:
amplitudeSource.out -> oscillatorBank.amplitudes;
```

**Current Oscen (workaround needed):**
```rust
// Can't pass [f32; 32] through streams or value endpoints cleanly
// Must manually wire in Voice::process() or use awkward value passing
```

## Current Architecture

### Stream Infrastructure

```rust
// In graph/types.rs
pub type StreamOutput = ValueKey;  // Just a key to scalar storage
pub type StreamInput = ValueKey;

// In graph/graph_impl.rs
context.stream(input_idx) -> f32  // Always returns scalar

// In ProcessingContext
pub fn stream(&self, index: usize) -> f32 {
    self.input_values[index]
}
```

### Node Endpoints

```rust
#[derive(Node)]
struct MyNode {
    #[input(stream)]
    input: f32,  // Always f32

    #[output(stream)]
    output: f32,  // Always f32
}
```

## Proposed Architecture

### 1. Multi-Channel Stream Types

Add generic stream types that carry fixed-size arrays:

```rust
// In graph/types.rs

/// Stream channel data - can be scalar or multi-channel
#[derive(Clone, Debug)]
pub enum StreamData {
    Mono(f32),
    Multi(Vec<f32>),  // Or SmallVec for stack allocation
}

impl StreamData {
    pub fn mono(value: f32) -> Self {
        Self::Mono(value)
    }

    pub fn multi<const N: usize>(values: [f32; N]) -> Self {
        Self::Multi(values.to_vec())
    }

    pub fn as_mono(&self) -> f32 {
        match self {
            Self::Mono(v) => *v,
            Self::Multi(v) => v.get(0).copied().unwrap_or(0.0),
        }
    }

    pub fn as_multi<const N: usize>(&self) -> [f32; N] {
        match self {
            Self::Mono(v) => [*v; N],
            Self::Multi(v) => {
                let mut arr = [0.0; N];
                arr[..v.len().min(N)].copy_from_slice(&v[..v.len().min(N)]);
                arr
            }
        }
    }

    pub fn num_channels(&self) -> usize {
        match self {
            Self::Mono(_) => 1,
            Self::Multi(v) => v.len(),
        }
    }
}
```

### 2. Endpoint Type Annotations

Extend the macro syntax to support channel counts:

```rust
#[derive(Node)]
struct OscillatorBank {
    #[input(stream)]
    frequency: f32,  // Backward compatible: single channel

    #[input(stream<32>)]
    amplitudes: [f32; 32],  // Multi-channel stream

    #[output(stream)]
    output: f32,
}
```

**graph! macro syntax:**
```rust
graph! {
    name: Voice;

    output stream audio;           // Single channel (f32)
    output stream<32> amplitudes;  // 32 channels ([f32; 32])

    node {
        amp_source = AmplitudeSource::new();
        osc_bank = OscillatorBank::new(sample_rate);
    }

    connection {
        amp_source.amplitudes -> osc_bank.amplitudes;  // 32-channel connection
        osc_bank.output -> audio;                       // Single channel
    }
}
```

### 3. Graph Storage Changes

**Current:**
```rust
// In graph_impl.rs
struct NodeData {
    stream_inputs: Vec<f32>,
    stream_outputs: Vec<f32>,
}
```

**Proposed:**
```rust
struct NodeData {
    stream_inputs: Vec<StreamData>,   // Can be mono or multi-channel
    stream_outputs: Vec<StreamData>,
}
```

### 4. ProcessingContext Updates

```rust
impl ProcessingContext {
    // Backward compatible: get scalar from first channel
    pub fn stream(&self, index: usize) -> f32 {
        self.input_values[index].as_mono()
    }

    // New: get multi-channel data
    pub fn stream_multi<const N: usize>(&self, index: usize) -> [f32; N] {
        self.input_values[index].as_multi()
    }
}
```

### 5. NodeIO Trait Updates

```rust
pub trait NodeIO {
    fn read_inputs<'a>(&mut self, context: &mut ProcessingContext<'a>);

    // Existing
    fn get_stream_output(&self, index: usize) -> Option<f32> { None }
    fn set_stream_input(&mut self, index: usize, value: f32) {}

    // New multi-channel support
    fn get_stream_output_multi(&self, index: usize) -> Option<StreamData> { None }
    fn set_stream_input_multi(&mut self, index: usize, value: StreamData) {}
}
```

### 6. Macro Code Generation

The `#[derive(Node)]` macro would detect array types and generate appropriate code:

```rust
// For #[input(stream<32>)] amplitudes: [f32; 32]
impl NodeIO for OscillatorBank {
    fn set_stream_input_multi(&mut self, index: usize, value: StreamData) {
        match index {
            0 => self.amplitudes = value.as_multi::<32>(),
            _ => {}
        }
    }
}

// Auto-generated read_inputs
fn read_inputs(&mut self, context: &mut ProcessingContext) {
    self.frequency = context.stream(0);
    self.amplitudes = context.stream_multi::<32>(1);
}
```

## Implementation Plan

### Phase 1: Core Infrastructure (Week 1)
- [ ] Add `StreamData` enum to `graph/types.rs`
- [ ] Update `EndpointState` to store `StreamData` instead of scalar
- [ ] Update `ProcessingContext` with `stream_multi()` method
- [ ] Add `NodeIO` methods for multi-channel I/O
- [ ] Write unit tests for `StreamData` conversions

### Phase 2: Node Macro Support (Week 1-2)
- [ ] Parse `#[input(stream<N>)]` and `#[output(stream<N>)]` syntax
- [ ] Detect array field types `[f32; N]`
- [ ] Generate `set_stream_input_multi()` for array stream inputs
- [ ] Generate `get_stream_output_multi()` for array stream outputs
- [ ] Generate appropriate `read_inputs()` code using `stream_multi()`
- [ ] Add tests for multi-channel node macro generation

### Phase 3: Graph Macro Support (Week 2)
- [ ] Parse `output stream<N>` syntax in graph! macro
- [ ] Validate channel count matches in connections
- [ ] Generate multi-channel connection routing code
- [ ] Update `generate_signal_processor_impl()` for multi-channel streams
- [ ] Add tests for multi-channel graph generation

### Phase 4: Graph Processing (Week 2-3)
- [ ] Update `Graph::process()` to handle `StreamData`
- [ ] Update connection routing to copy multi-channel data
- [ ] Ensure backward compatibility with scalar streams
- [ ] Performance profiling (array copies vs. scalar)
- [ ] Add integration tests

### Phase 5: ElectricPiano Refactor (Week 3)
- [ ] Refactor `AmplitudeSource` to output `stream<32>`
- [ ] Refactor `OscillatorBank` to input `stream<32>`
- [ ] Update `Voice` graph to use multi-channel connections
- [ ] Verify audio output matches previous implementation
- [ ] Performance comparison vs. CMajor

### Phase 6: Documentation & Examples (Week 3-4)
- [ ] Update architecture docs
- [ ] Add multi-channel stream examples
- [ ] Migration guide for existing code
- [ ] API documentation

## Backward Compatibility

### Strategy: Graceful Migration

All existing code using scalar streams continues to work:

```rust
// Old code (still works)
#[derive(Node)]
struct Filter {
    #[input(stream)]
    input: f32,  // Treated as stream<1>
}

// New code (opt-in)
#[derive(Node)]
struct Mixer {
    #[input(stream<8>)]
    inputs: [f32; 8],
}
```

### Automatic Conversions

- Scalar `f32` → `StreamData::Mono(f32)`
- `[f32; 1]` ↔ `f32` (implicit conversion)
- Connecting mono to multi: broadcast to all channels
- Connecting multi to mono: sum or take first channel

## Performance Considerations

### Memory Layout

**Option A: Vec<f32> (Dynamic)**
- Pro: Flexible channel count
- Con: Heap allocation, cache misses

**Option B: SmallVec<[f32; 32]> (Hybrid)**
- Pro: Stack allocation for ≤32 channels
- Con: Fixed max size

**Option C: Const Generic [f32; N]**
- Pro: Zero-cost, stack allocated
- Con: Requires const generics everywhere

**Recommendation:** Start with SmallVec for flexibility, optimize later if needed.

### Copy Overhead

Multi-channel streams require array copies during routing. Mitigation:
- Use SIMD for large arrays (32 channels = 128 bytes → 2 AVX loads)
- Consider Arc<[f32]> for read-only multi-tap routing
- Profile hot paths and optimize as needed

## Alternative Approaches Considered

### 1. Value Endpoints with Arrays
**Current workaround:** Use value inputs/outputs for `[f32; N]`

**Problems:**
- Node macro doesn't handle non-scalar values well
- Semantically wrong (control-rate vs. audio-rate)
- Requires manual ValueRef unwrapping

### 2. Multiple Scalar Streams
**Approach:** Use `[StreamInput; 32]` instead of multi-channel

**Problems:**
- Clutters endpoint definitions
- 32 separate connections in graph
- Inefficient routing (32 lookups vs. 1)

### 3. Keep Scalar Streams, Manual Wiring
**Approach:** Don't add multi-channel support, wire manually in process()

**Problems:**
- Defeats graph architecture
- Can't visualize multi-channel flow
- Not composable

## Examples After Implementation

### Stereo Panner
```rust
#[derive(Node)]
struct StereoPanner {
    #[input(stream)]
    mono_in: f32,

    #[input(value)]
    pan: f32,  // -1 (left) to +1 (right)

    #[output(stream<2>)]
    stereo_out: [f32; 2],
}

impl SignalProcessor for StereoPanner {
    fn process(&mut self, _: f32) {
        let l_gain = ((1.0 - self.pan) * 0.5).sqrt();
        let r_gain = ((1.0 + self.pan) * 0.5).sqrt();

        self.stereo_out = [
            self.mono_in * l_gain,
            self.mono_in * r_gain,
        ];
    }
}
```

### FFT Processor
```rust
#[derive(Node)]
struct SpectralProcessor {
    #[input(stream)]
    audio_in: f32,

    #[output(stream<1024>)]
    fft_bins: [f32; 1024],
}
```

### Modulation Matrix
```rust
graph! {
    name: ModMatrix;

    output stream<4> mod_outputs;  // 4 modulation sources

    node {
        lfo1 = LFO::new();
        lfo2 = LFO::new();
        env1 = Envelope::new();
        env2 = Envelope::new();
        combiner = ModCombiner::new();  // Combines into stream<4>
    }

    connection {
        lfo1.output -> combiner.input_0;
        lfo2.output -> combiner.input_1;
        env1.output -> combiner.input_2;
        env2.output -> combiner.input_3;

        combiner.output -> mod_outputs;
    }
}
```

## Open Questions

1. **Should we support variable-length streams?** (e.g., `stream<N>` where N is runtime)
   - Pro: Maximum flexibility (dynamic routing, FFT sizes)
   - Con: Performance overhead, complexity

2. **How to handle channel count mismatches?**
   - Error at compile time? (type safety)
   - Auto-convert at runtime? (flexibility)
   - Current proposal: Runtime validation with automatic mixing/splitting

3. **Should stereo be special-cased?**
   - Option: `stream stereo` as sugar for `stream<2>`
   - Option: Keep explicit `stream<2>` for consistency

4. **SIMD optimization strategy?**
   - When to use SIMD for array copies?
   - Should StreamData internally use SIMD types?

## Success Metrics

- ✅ ElectricPiano refactor uses clean multi-channel architecture
- ✅ No performance regression vs. current scalar implementation
- ✅ Backward compatible with all existing examples
- ✅ CMajor-equivalent expressiveness for multi-channel routing
- ✅ Zero-cost abstractions (release builds as fast as manual code)

## References

- [CMajor Stream Documentation](https://cmajor.dev/docs/Language/#streams)
- [Rust Const Generics](https://doc.rust-lang.org/reference/items/generics.html#const-generics)
- [SmallVec](https://docs.rs/smallvec/)
