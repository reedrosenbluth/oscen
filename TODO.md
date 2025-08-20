### Small todos
- slotmap capacity
- replace ArrayVecs with SlotMaps?
- try implementing a few more SignalProcessors (mixer...)
- implement multiple outputs for SignalProcessor
- fix value smoothing
  - to account for larger ranges (like filter) (i don't remember what this means?)
  - chatgpt found an error with my smoothing i should investigate furtuer
- fix ring buffer, there's a bug
- investigate oscillator amplitude modulation

### Big todos
- topological sorting the dag
- investigate Graph implementing SignalProcessor
- windowed sinc interpolation for buffer


notes
- node sorting/processing
  - mark & sweep
- feedback
-
