# Cmajor Performance Benchmarks

This directory contains Cmajor implementations of oscen benchmarks for performance comparison with JUCE and oscen.

## Overview

These benchmarks measure pure DSP performance using the Cmajor JIT-compiled audio language. They match the exact same workloads as the JUCE and oscen benchmarks for direct comparison.

## Benchmarks

### simple-sine

A single sine wave oscillator running at 440Hz.

**Components:**
- 1 sine oscillator (440Hz)

**Matches:**
- oscen's `simple_graph`
- JUCE's `simple-sine`

### medium-graph

Medium complexity audio graph.

**Components:**
- 2 oscillators (sine 440Hz + saw 442Hz)
- 1 lowpass filter (1000Hz, Q=0.7)
- 1 ADSR envelope (0.01, 0.1, 0.7, 0.2)
- Mixer and multiplier for routing

**Matches:**
- oscen's `medium_graph`
- JUCE's `medium-graph`

### complex-graph

Complex audio graph with multiple signal paths.

**Components:**
- 5 oscillators (alternating sine/saw at 440Hz, 450Hz, 460Hz, 470Hz, 480Hz)
- 2 lowpass filters (800Hz Q=0.5, 1200Hz Q=0.5)
- 2 ADSR envelopes with different parameters
- Delay (0.5s, 0.3 feedback)
- Multiple mixers and multipliers

**Matches:**
- oscen's `complex_graph`
- JUCE's `complex-graph`

## Building

### Prerequisites

- CMake 3.15 or higher
- C++17 compatible compiler
- Git (for fetching Cmajor)
- LLVM (Cmajor uses LLVM JIT)

### Build Instructions

```bash
cd cmajor-benchmarks
mkdir build && cd build
cmake .. -DCMAKE_BUILD_TYPE=Release
cmake --build . --config Release
```

**Note:** The first build will take a while as it fetches and builds the Cmajor engine and LLVM dependencies.

## Running Benchmarks

### Simple Sine

```bash
cd cmajor-benchmarks/build/simple-sine
./simple-sine
```

### Medium Graph

```bash
cd cmajor-benchmarks/build/medium-graph
./medium-graph
```

### Complex Graph

```bash
cd cmajor-benchmarks/build/complex-graph
./complex-graph
```

## Comparing with JUCE and Oscen

All three frameworks process the same 441,000 samples (10 seconds at 44.1kHz) and report:
- **Samples per second** - Higher is better
- **Real-time factor** - How many times faster than real-time
- **Microseconds per sample** - Lower is better

### Run all benchmarks:

```bash
# Cmajor
cd cmajor-benchmarks/build
./simple-sine/simple-sine
./medium-graph/medium-graph
./complex-graph/complex-graph

# JUCE
cd juce-benchmarks/build
./simple-sine/simple-sine_artefacts/Release/simple-sine
./medium-graph/medium-graph_artefacts/Release/medium-graph
./complex-graph/complex-graph_artefacts/Release/complex-graph

# Oscen
cargo build --release --bin oscen_bench
./target/release/oscen_bench
```

## About Cmajor

Cmajor is a JIT-compiled procedural language designed specifically for audio DSP. Key features:

- **JIT Compilation**: Uses LLVM to compile to native code at runtime
- **Graph-based**: Natural graph/processor model similar to Max/MSP
- **Type-safe**: Strong typing with compile-time checks
- **Optimized**: LLVM optimizations produce efficient machine code

The benchmarks use Cmajor's C++ API to load and execute `.cmajor` programs, measuring just the DSP performance without audio I/O overhead.

## Performance Characteristics

Expected performance characteristics compared to JUCE and oscen:

- **Cmajor**: JIT-compiled, should be close to native C++ performance after warm-up
- **JUCE**: Direct C++ compilation, optimized with -O3 and LTO
- **Oscen**: Rust compilation with release optimizations

The comparison reveals the overhead (if any) of different graph abstraction models:
- Cmajor's processor/graph model with JIT compilation
- JUCE's AudioProcessor/AudioProcessorGraph with C++ compilation
- Oscen's graph-based Rust architecture

## Notes

- All benchmarks use Release builds for accurate performance measurement
- Buffer size is 512 samples (matching JUCE/oscen benchmarks)
- Sample rate is 44.1kHz
- 441,000 samples processed (10 seconds of audio)
- Cmajor engine warm-up time is not included in measurements
