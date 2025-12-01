# Oscen [![crates.io](https://img.shields.io/crates/v/oscen.svg)](https://crates.io/crates/oscen)

<picture>
    <source media="(prefers-color-scheme: dark)" srcset="logo-dark.svg">
    <source media="(prefers-color-scheme: light)" srcset="logo-light.svg">
    <img src="logo-light.svg">
</picture>
<br />
<br />

Oscen _["oh-sin"]_ is a library for writing audio software in Rust.

At its core is a graph-based processing engine where nodes (oscillators, filters,
envelopes, effects) connect through typed endpoints. The `graph!` macro lets you
define synthesizers declaratively, with automatic topological sorting and
sample-by-sample processing.

## Example

```rust
use oscen::{graph, PolyBlepOscillator, TptFilter};

graph! {
    name: Synth;

    // Control inputs with defaults
    input value mod_freq = 5.0;
    input value mod_depth = 0.2;
    input value carrier_freq = 440.0;
    input value cutoff = 1200.0;

    // Audio output
    output stream audio_out;

    // Define nodes
    node {
        modulator = PolyBlepOscillator::sine(5.0, 0.2);
        carrier = PolyBlepOscillator::saw(440.0, 0.5);
        filter = TptFilter::new(1200.0, 0.707);
    }

    // Connect nodes
    connection {
        mod_freq -> modulator.frequency;
        mod_depth -> modulator.amplitude;
        carrier_freq -> carrier.frequency;
        cutoff -> filter.cutoff;
        modulator.output -> carrier.frequency_mod;
        carrier.output -> filter.input;
        filter.output -> audio_out;
    }
}
```
