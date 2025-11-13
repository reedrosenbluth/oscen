# Audio Output Fix Summary

## Problem

The electric-piano example was not producing audio output after the architectural changes to the graph system (struct-of-arrays I/O pattern in static-graph-2 branch).

## Root Cause

The `SignalProcessor` trait's `process()` method signature changed from:
```rust
fn process(&mut self, sample_rate: f32, context: &mut ProcessingContext);
```

to:
```rust
fn process(&mut self, sample_rate: f32);
```

Two custom nodes in the electric-piano example (`ElectricPianoVoiceNode` and `Tremolo`) were still using the OLD signature with the `ProcessingContext` parameter. This meant:
1. Their `process()` methods were NOT implementing the trait
2. They were just regular methods with a similar name that never got called
3. The nodes never produced any audio output

## New Architecture Pattern

With the struct-of-arrays I/O pattern, nodes should:

### 1. Read inputs from fields directly
The `#[derive(Node)]` macro automatically generates a `read_inputs()` method that populates input fields from the graph before `process()` is called.

**OLD (incorrect):**
```rust
fn process(&mut self, _sample_rate: f32, context: &mut ProcessingContext) {
    let frequency = self.get_frequency(context);
    let rate = self.get_rate(context);
    // ...
}
```

**NEW (correct):**
```rust
fn process(&mut self, _sample_rate: f32) {
    // Read directly from fields (already populated by read_inputs())
    let frequency = self.frequency;
    let rate = self.rate;
    // ...
}
```

### 2. Handle events via on_<field_name>() methods
For event inputs, implement a handler method that the macro will automatically dispatch to:

```rust
impl MyNode {
    // Event handler called automatically by the macro-generated NodeIO
    fn on_gate(&mut self, event: &EventInstance, _context: &mut ProcessingContext) {
        match &event.payload {
            EventPayload::Scalar(velocity) if *velocity > 0.0 => {
                // Handle note on
            }
            _ => {
                // Handle note off
            }
        }
    }
}
```

## Files Fixed

### Electric Piano Example
1. **examples/electric-piano/src/electric_piano_voice.rs**
   - Updated `SignalProcessor::process()` to remove `context` parameter
   - Changed to read from `self.field_name` directly
   - Implemented `on_gate()` handler method for event processing

2. **examples/electric-piano/src/tremolo.rs**
   - Updated `SignalProcessor::process()` to remove `context` parameter
   - Changed to read from `self.field_name` directly

### Helper Nodes (Critical - Affects ALL Examples)
3. **oscen-lib/src/graph/helpers.rs**
   - **FunctionNode**: Was completely broken (process() was a no-op)
     - Added input/output fields
     - Stored function pointer
     - Implemented proper process() that applies the function
     - Implemented read_inputs(), get_stream_output(), set_stream_input()

   - **BinaryFunctionNode**: Was completely broken (process() was a no-op)
     - Added lhs/rhs/output fields
     - Stored function pointer
     - Implemented proper process() that applies the binary function
     - Implemented read_inputs(), get_stream_output(), set_stream_input()

4. **oscen-lib/src/graph/graph_impl.rs**
   - Fixed BinaryFunctionNode stream input count (was 1, should be 2)

**Impact**: These helper nodes are used by `Graph::transform()` and `Graph::combine()`. Without this fix, the supersaw example (and any other code using these methods) would produce complete silence!

## Status

- ✅ ElectricPianoVoiceNode fixed
- ✅ Tremolo fixed
- ✅ FunctionNode fixed (critical - was completely non-functional)
- ✅ BinaryFunctionNode fixed (critical - was completely non-functional)
- ✅ Both examples should now produce audio
- ⚠️ Build verification blocked by missing system library (alsa-sys) in test environment

## Testing

To test the fix:
1. Build the electric-piano example: `cargo build --release -p electric-piano`
2. Run it and play notes via MIDI keyboard
3. Audio should now be produced correctly

The supersaw example should also work correctly as it was already using the correct pattern.
