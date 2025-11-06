# Testing Compile-Time Graphs: Guide and Benchmarks

## Can compile-time graphs be built without the macro?

**Yes!** The macro is just a code generator - it produces regular Rust code. You can write that code by hand to:

1. **Test the concept before the macro is fully working**
2. **Understand what the macro generates**
3. **Create custom optimizations beyond what the macro provides**
4. **Debug issues with the macro-generated code**

## What does a hand-written compile-time graph look like?

Here's a simple example (osc -> filter -> gain):

```rust
pub struct HandWrittenSynth {
    // Direct node fields (not Box<dyn SignalProcessor>)
    osc: Oscillator,
    filter: TptFilter,
    gain: Gain,

    // IO structs for each node (persistent, stack-allocated)
    osc_io: OscillatorIO,
    filter_io: TptFilterIO,
    gain_io: GainIO,

    sample_rate: f32,
}

impl HandWrittenSynth {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            osc: Oscillator::sine(440.0, 1.0),
            filter: TptFilter::new(1000.0, 0.7),
            gain: Gain::new(0.5),
            osc_io: Default::default(),
            filter_io: Default::default(),
            gain_io: Default::default(),
            sample_rate,
        }
    }

    #[inline]
    pub fn process(&mut self) -> f32 {
        // Direct field assignments (compile-time connections)
        self.filter_io.input = self.osc_io.output;
        self.gain_io.input = self.filter_io.output;

        // Direct method calls (no dynamic dispatch)
        self.osc.process_internal(&mut self.osc_io, self.sample_rate);
        self.filter.process_internal(&mut self.filter_io, self.sample_rate);
        self.gain.process_internal(&mut self.gain_io, self.sample_rate);

        self.gain_io.output
    }
}
```

**Key differences from runtime graphs:**

| Feature | Runtime Graph | Hand-Written Compile-Time |
|---------|--------------|---------------------------|
| Node storage | `Vec<Box<dyn SignalProcessor>>` | Direct struct fields |
| Node lookup | SlotMap index | Direct field access |
| Method calls | Virtual/dynamic dispatch | Direct calls (inlineable) |
| Connections | Runtime connection map | Compile-time field assignments |
| Memory | Heap allocated | Stack allocated |
| Can inline? | No (virtual calls) | Yes (compiler sees everything) |

## How to test in benchmarks

We've created a comprehensive benchmark suite in `oscen-lib/benches/compile_time_bench.rs` that compares:

### 1. Runtime Graph (baseline)
```rust
fn runtime_synth() -> Graph {
    let mut graph = Graph::new(44100.0);
    let osc = graph.add_node(Oscillator::sine(440.0, 1.0));
    let filter = graph.add_node(TptFilter::new(1000.0, 0.7));
    let gain = graph.add_node(Gain::new(0.5));
    graph.connect(osc.output, filter.input);
    graph.connect(filter.output, gain.input);
    graph
}

// Benchmark
let mut graph = runtime_synth();
graph.validate().unwrap();
b.iter(|| {
    black_box(graph.process().unwrap());
});
```

### 2. Hand-Written Compile-Time
```rust
let mut synth = HandWrittenSynth::new(44100.0);
b.iter(|| {
    black_box(synth.process());
});
```

### 3. Macro-Generated Compile-Time (future)
```rust
graph! {
    name: MacroGeneratedSynth;
    mode: CompileTime;
    nodes { /* ... */ }
    connections { /* ... */ }
}

let mut synth = MacroGeneratedSynth::new(44100.0);
b.iter(|| {
    black_box(synth.process());
});
```

## Running the benchmarks

```bash
# Run all benchmarks
cargo bench

# Run just compile-time vs runtime comparison
cargo bench --bench compile_time_bench

# Run with more detail
cargo bench --bench compile_time_bench -- --verbose

# Compare specific benchmarks
cargo bench --bench compile_time_bench "synth_comparison"

# Get HTML report
cargo bench --bench compile_time_bench -- --save-baseline main
```

### Expected Results

Based on similar systems (CMajor, Gen~, FAUST), we expect:

**Single Sample Processing:**
- Runtime graph: ~100-150ns per sample (baseline)
- Hand-written compile-time: ~5-10ns per sample (10-20x faster)
- Macro-generated compile-time: ~5-10ns per sample (same as hand-written)

**Batch Processing (512 samples):**
- Runtime graph: ~60-80μs per batch
- Hand-written compile-time: ~3-5μs per batch (15-20x faster)

**Why the speedup?**

1. **No SlotMap lookups** (~10-20ns saved per node)
2. **No dynamic dispatch** (~5-10ns saved per node call)
3. **Full inlining** (LLVM can optimize across node boundaries)
4. **No Result wrapping** (no error handling overhead)
5. **Better cache locality** (stack-allocated, contiguous memory)

## Analyzing the generated assembly

To see what the compiler generates:

```bash
# For hand-written version
cargo rustc --release --bench compile_time_bench -- --emit asm

# Look at the assembly
cat target/release/deps/*.s | grep -A 50 "HandWrittenSynth::process"
```

You should see:
- **Runtime version**: Lots of calls through function pointers, SlotMap operations
- **Compile-time version**: Tight inline code, direct math operations, no calls

## Benchmarking best practices

1. **Use `black_box()`** to prevent dead code elimination
   ```rust
   black_box(synth.process());  // Compiler can't optimize away
   ```

2. **Warm up caches** before measuring
   ```rust
   for _ in 0..1000 { synth.process(); }  // Warmup
   b.iter(|| { black_box(synth.process()); });
   ```

3. **Test realistic scenarios**
   - Single sample (latency)
   - Batch processing (throughput)
   - Different graph complexities (scalability)

4. **Compare apples to apples**
   - Same nodes in both versions
   - Same sample rate
   - Same parameter values
   - Same connection topology

## Current Limitations

The hand-written version in our benchmark is simplified because:

1. **IO structs may not be exported** from node modules yet
2. **`process_internal()` methods** don't exist yet (nodes still use `process()`)
3. **Connections aren't wired** (nodes process independently)

### To get full compile-time performance, we need:

1. **Export IO structs** from node modules:
   ```rust
   // In oscen-lib/src/oscillators/mod.rs
   pub use self::{Oscillator, OscillatorIO, OscillatorEndpoints};
   ```

2. **Add `process_internal()` methods** to nodes:
   ```rust
   impl Oscillator {
       #[inline]
       pub fn process_internal(&mut self, io: &mut OscillatorIO, sample_rate: f32) {
           // Process using IO struct directly
           io.output = /* ... */;
       }
   }
   ```

3. **Wire connections** in the hand-written benchmark

## Next Steps

1. **Export IO structs** from modules
2. **Add `process_internal()` to a few nodes** (Oscillator, Gain, TptFilter)
3. **Update hand-written benchmark** to use real connections
4. **Run benchmarks** and verify speedup
5. **Complete macro implementation** to match hand-written performance
6. **Add macro-generated version** to benchmarks

## Validating the macro

Once the macro is working, validate it generates the same code as hand-written:

```bash
# Expand macro
cargo expand --lib > expanded.rs

# Search for the generated struct
grep -A 100 "pub struct MacroGeneratedSynth" expanded.rs

# Compare with hand-written version
# Should be nearly identical!
```

## Performance Testing Checklist

- [ ] Benchmark runtime graph (baseline)
- [ ] Benchmark hand-written compile-time graph
- [ ] Verify 10-20x speedup
- [ ] Add `process_internal()` methods to nodes
- [ ] Wire up connections in hand-written version
- [ ] Re-benchmark with connections
- [ ] Implement macro compile-time generation
- [ ] Benchmark macro-generated version
- [ ] Verify macro matches hand-written performance
- [ ] Test with complex graphs (polysynth)
- [ ] Profile with perf/cachegrind
- [ ] Analyze generated assembly
