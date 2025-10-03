### Small todos
- slotmap capacity
- replace ArrayVecs with SlotMaps?
- implement multiple outputs for SignalProcessor
- investigate oscillator amplitude modulation
- use phantom data to mark input vs output endpoint, and consolidate types
- consider different API for defining nodes
  - maybe all info should directly get passed to macro
- look into error crates (anyhow and thiserror) for errors
- audio thread allocation fixes
  - AdsrEnvelope::process clones gate events into a new Vec every audio callback
  - The graph stores pending_events: Vec<PendingEvent> initialized with Vec::new(). The first emitted event on the audio thread will grow this buffer, causing an allocation in realtime code. this can be preallocated
  -

### Big todos
- multi-output nodes
- investigate Graph implementing SignalProcessor
  - graph flattening
  - more explicit endpoint declarations/hoisting
- windowed sinc interpolation for buffer
