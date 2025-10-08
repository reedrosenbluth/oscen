### Small todos
- slotmap capacity
- replace ArrayVecs with SlotMaps?
- implement multiple outputs for SignalProcessor
- investigate oscillator amplitude modulation
- consider different API for defining nodes
  - maybe all info should directly get passed to macro
- look into error crates (anyhow and thiserror) for errors
- Graph::connect always pushes the wiring without verifying that the source/destination endpoint types are compatible, so wiring errors fail silently at runtime.
- create oscen prelude

### Big todos
- multi-output nodes
- investigate Graph implementing SignalProcessor
  - more explicit endpoint declarations/hoisting
- graph flattening
  -
- windowed sinc interpolation for buffer
- simd optimizations?
