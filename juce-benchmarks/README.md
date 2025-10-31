# JUCE Performance Benchmarks

This directory contains JUCE implementations of oscen benchmarks for performance comparison.

## Overview

These benchmarks measure pure DSP performance (no audio I/O) to compare JUCE's audio processing framework with oscen's graph-based architecture.

## Benchmarks

### simple-sine

A single sine wave oscillator running at 440Hz. This is the baseline benchmark that matches oscen's `simple_graph` from `oscen-lib/benches/graph_bench.rs`.

**Components:**
- 1 sine oscillator (440Hz, JUCE dsp::Oscillator)

**Oscen equivalent:**
```rust
let mut graph = Graph::new(44100.0);
let osc = graph.add_node(Oscillator::sine(440.0, 1.0));
```

## Building

### Prerequisites

- CMake 3.15 or higher
- C++17 compatible compiler
- Git (for fetching JUCE)

### Build Instructions

```bash
cd juce-benchmarks/simple-sine
mkdir build && cd build
cmake .. -DCMAKE_BUILD_TYPE=Release
cmake --build . --config Release
```

## Running Benchmarks

### Simple Sine

```bash
cd juce-benchmarks/simple-sine/build
./simple-sine
```

Expected output:
```
=== JUCE Simple Sine (1 oscillator) ===
Processing 441000 samples...
Processed 441000 samples in XXXXX microseconds
Samples per second: XXXXXXX.XX
Real-time factor: XXX.XXx
Microseconds per sample: X.XX
```

## Comparing with Oscen

### Run oscen benchmark:

```bash
# From oscen root
cargo build --release --bin profile_graph
./target/release/profile_graph
```

### Run JUCE benchmark:

```bash
# From juce-benchmarks/simple-sine/build
./simple-sine
```

### Metrics to compare:

- **Samples per second** - Higher is better
- **Real-time factor** - How many times faster than real-time (e.g., 100x means it can process 100s of audio in 1s)
- **Microseconds per sample** - Lower is better

## Profiling

### Using perf (Linux)

```bash
# JUCE
perf record --call-graph=dwarf ./simple-sine
perf report

# Oscen
cd oscen-lib
cargo build --release --bin profile_graph
perf record --call-graph=dwarf ../target/release/profile_graph
perf report
```

### Using flamegraph (Linux)

```bash
# Install flamegraph
cargo install flamegraph

# JUCE - use perf directly
perf record --call-graph=dwarf -F 999 ./simple-sine
perf script | stackcollapse-perf.pl | flamegraph.pl > juce-simple-sine.svg

# Oscen
cd oscen-lib
cargo flamegraph --bin profile_graph
```

## Future Benchmarks

Planned additions to match oscen's benchmark suite:

- **medium-graph** - 2 oscillators + filter + envelope
- **complex-graph** - 5 oscillators + 2 filters + 2 envelopes + delay
- **polysynth** - Polyphonic synthesizer with multiple voices

## Notes

- All benchmarks use Release builds for accurate performance measurement
- Buffer size is fixed at 512 samples to match oscen examples
- Sample rate is 44.1kHz
- Benchmarks process 441,000 samples (10 seconds of audio) for consistent measurement
