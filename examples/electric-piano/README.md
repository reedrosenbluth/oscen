# Electric Piano Example

A polyphonic electric piano synthesizer with 16 voices and real-time MIDI input, demonstrating Oscen's capabilities for complex audio synthesis.

## Features

- **16-voice polyphony** with voice allocation
- **32 harmonics per voice** with per-harmonic envelopes
- **Real-time MIDI input** via `midir`
- **Interactive GUI** built with Slint
- **Adjustable parameters**:
  - Brightness
  - Velocity Scaling
  - Decay Rate
  - Harmonic Decay
  - Key Scaling
  - Release Rate
  - Vibrato Intensity
  - Vibrato Speed

## Architecture

The synthesizer is built using Oscen's `graph!` macro for compile-time graph generation:

### Voice Architecture
Each voice contains:
- `ElectricPianoVoiceNode`: Combined oscillator bank with 32 harmonics
- Per-harmonic envelope generators
- Velocity-sensitive amplitude control
- Key-scaling for realistic piano timbre

### Main Graph
- **MIDI Parser**: Converts raw MIDI to note events
- **Voice Allocator**: Distributes notes across 16 voices
- **16 Voice Handlers**: Track note on/off and convert to frequency/gate
- **16 Electric Piano Voices**: Full synthesis per voice
- **Tremolo Effect**: Stereo modulation effect
- **Stereo Output**: Mixed and processed audio

## Running

```bash
cargo run --release --example electric-piano
```

**Note:** Requires MIDI input device. The synth will work without MIDI but you'll need to connect a MIDI keyboard or controller to play notes.

## Performance

This example demonstrates the need for high-performance audio processing:
- 16 voices × 32 harmonics = **512 oscillators**
- 16 voices × 32 envelopes = **512 envelope generators**
- Real-time processing at 44.1kHz with 512-sample buffers
- ~11ms latency (512 samples @ 44.1kHz)

### Stack Size Note

The example requires an 8MB stack size (default is 2MB) due to the large harmonic arrays:

```rust
thread::Builder::new()
    .stack_size(8 * 1024 * 1024)
    .spawn(move || {
        // Audio processing...
    })
```

## Future: JIT Compilation Support

⚡ **JIT support is coming!**

Once JIT code generation is implemented for custom nodes like `ElectricPianoVoiceNode`, this example will benefit from:

- **10-20x performance improvement**
- Lower CPU usage
- Potential for more voices or higher harmonic count
- Even lower latency

### Why Not JIT Yet?

The current JIT implementation supports only basic nodes (Oscillator, Gain). Custom nodes like `ElectricPianoVoiceNode` require:

1. **Custom code generation** for the 32-harmonic oscillator bank
2. **Fallback support** for interpreted execution
3. **SIMD optimizations** for harmonic summation

These features are planned for future releases.

### How To Enable JIT (Future)

Once supported, enabling JIT will be simple:

```rust
// Option 1: Convert graph! output to JIT
let jit_graph = JITGraph::from_graph(synth.graph);

// Option 2: New JIT-enabled graph! macro variant
jit_graph! {
    name: ElectricPianoGraph;
    compile: runtime; // Enables JIT
    // ... rest of graph definition
}
```

See the [`jit-demo`](../jit-demo/) example for current JIT capabilities.

## Code Structure

- `main.rs` - Application entry, audio callback, UI setup
- `electric_piano_voice.rs` - 32-harmonic voice node
- `harmonic_envelope.rs` - Per-harmonic envelope generator (unused in current architecture)
- `harmonic_bank.rs` - Oscillator bank for harmonics (unused in current architecture)
- `tremolo.rs` - Stereo tremolo/vibrato effect
- `midi_input.rs` - MIDI input handling via midir
- `ui/synth_window.slint` - GUI definition

## Performance Tips

1. **Use Release Mode**: `cargo run --release`
   - Debug builds are 10-100x slower
   - Essential for real-time audio

2. **Adjust Buffer Size**: Modify in `main.rs`
   ```rust
   buffer_size: cpal::BufferSize::Fixed(512), // Try 256, 512, 1024
   ```
   - Smaller = lower latency, higher CPU
   - Larger = more latency, lower CPU

3. **Profile Hot Paths**:
   ```bash
   cargo build --release --example electric-piano
   perf record -g ./target/release/examples/electric-piano
   perf report
   ```

4. **Monitor CPU Usage**:
   - Should be <10% on modern CPUs
   - If higher, consider reducing voice count or harmonics

## Comparison to CMajor

This implementation matches the architecture of the CMajor electric piano example:

| Feature | CMajor | Oscen |
|---------|--------|-------|
| Voices | 16 | 16 ✅ |
| Harmonics | 32 | 32 ✅ |
| Per-harmonic envelopes | Yes | Yes ✅ |
| Velocity scaling | Yes | Yes ✅ |
| Real-time MIDI | Yes | Yes ✅ |
| JIT compilation | Yes | Coming soon ⏳ |
| Performance | ~5-10% CPU | ~15-20% CPU (interpreted) |

With JIT compilation, Oscen will match CMajor's performance!

## Troubleshooting

### No MIDI Device Found
```
Error: No MIDI input ports available
```
- Connect a MIDI keyboard or controller
- Check OS MIDI permissions
- Try `midir` test utilities

### Audio Glitches/Dropouts
- Increase buffer size to 1024
- Use release mode
- Close other audio applications
- Check CPU usage

### Stack Overflow
```
thread 'audio' has overflowed its stack
```
- The example already uses 8MB stack
- If still happening, reduce `NUM_HARMONICS` in `electric_piano_voice.rs`

### Compilation Errors
```
error: no method named `process` found
```
- Ensure you're using the latest Oscen from this branch
- The struct-of-arrays refactoring changed the API

## Related Examples

- [`jit-demo`](../jit-demo/) - JIT compilation demonstration
- [`medium-graph`](../medium-graph/) - Graph building patterns
- Various simple examples in `examples/` directory

## Credits

Based on the CMajor electric piano example, demonstrating how Oscen provides similar capabilities with Rust's safety and ergonomics.
