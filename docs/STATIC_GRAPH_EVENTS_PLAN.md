# Implementation Plan: Event Support for Static Graphs

## Overview
Add zero-overhead event support to static/compile-time graphs using stack-allocated fixed-capacity event queues and a lightweight StaticContext for event emission.

**Design Decisions:**
- **Priority**: Performance-first with zero overhead
- **Context Approach**: Lightweight StaticContext for event emission
- **Compatibility**: Accept breaking changes (nodes must be adapted)
- **Storage**: ArrayVec for fixed-capacity stack-allocated queues

## Phase 1: Core Infrastructure (Foundation) ✅ COMPLETED

### 1.1 Add StaticContext type ✅
- **File**: `oscen-lib/src/graph/static_context.rs` (new)
- **Status**: ✅ DONE
- Created lightweight context with:
  ```rust
  pub struct StaticContext<'a> {
      pending_events: &'a mut ArrayVec<PendingEvent, 64>,
  }
  ```
- Implemented `emit_event()`, `emit_scalar_event()`, `emit_timed_event()` methods
- Added unit tests for event emission and queue overflow behavior
- Exported from `oscen-lib/src/graph/mod.rs`
- **Notes**:
  - Removed `Copy` trait from `PendingEvent` (EventInstance doesn't implement Copy)
  - Queue overflow panics in debug, silently drops in release

### 1.2 Update event types for static graphs ✅
- **File**: `oscen-lib/src/graph/types.rs`
- **Status**: ✅ DONE
- Added type alias: `pub type StaticEventQueue = ArrayVec<EventInstance, MAX_STATIC_EVENTS_PER_ENDPOINT>;`
- Added constant: `pub const MAX_STATIC_EVENTS_PER_ENDPOINT: usize = 32;`
- Exported from `oscen-lib/src/graph/mod.rs`

### 1.3 Add ArrayVec dependency ✅
- **File**: `oscen-lib/Cargo.toml`
- **Status**: ✅ DONE (already present at version 0.7.6)

## Phase 2: Static Graph Code Generation (PARTIALLY COMPLETE)

### 2.1 Generate event input/output fields ✅
- **File**: `oscen-macros/src/graph_macro/codegen.rs`
- **Status**: ✅ DONE
- Updated `generate_static_struct()` (line ~1656):
  - Event inputs now generate: `pub input_name: StaticEventQueue`
  - Event outputs now generate: `pub output_name: StaticEventQueue`
- Updated `generate_static_input_params()` (line ~351):
  - Event inputs initialized with: `let name = StaticEventQueue::new();`
- Updated `generate_static_output_params()` (line ~386):
  - Event outputs initialized with: `let name = StaticEventQueue::new();`
- **Result**: Static graph structs now have proper event queue fields

### 2.2 Add event queue clearing ✅
- **File**: `oscen-macros/src/graph_macro/codegen.rs`
- **Status**: ✅ DONE
- Updated `generate_static_process()` (line ~1631):
  - Added clearing of graph input/output event queues after processing
  - Prevents event accumulation across frames
- **Note**: Full event handling still pending (depends on Phase 3)

### 2.3 Update static process() for event handling ⏳ BLOCKED
- **File**: `oscen-macros/src/graph_macro/codegen.rs`
- **Status**: ⏳ BLOCKED (waiting on Phase 3)
- **Blockers**:
  - Nodes need event storage fields (Phase 3.1)
  - Nodes need `handle_events()` method (Phase 3.2)
- **Plan**:
  1. Create pending_events ArrayVec
  2. For each node:
     a. Route input events from graph/other nodes
     b. Create StaticContext with pending_events
     c. Call `node.handle_events(&mut ctx)`
     d. Call `node.process()`
     e. Route pending_events to connected nodes
     f. Clear node input queues

### 2.4 Generate event routing code ⏳ BLOCKED
- **Status**: ⏳ BLOCKED (waiting on Phase 2.3)
- Need to analyze connections for event endpoints
- Generate inline event forwarding between nodes
- Will be implemented as part of 2.3

## Phase 3: Node Macro Updates ✅ PARTIAL / ⏳ APPROACH DECIDED

### 3.1 Macro approach decided ✅
- **File**: `oscen-macros/src/lib.rs`
- **Status**: ✅ APPROACH FINALIZED
- **Decision**: NO special attribute needed - same `#[derive(Node)]` works for both
- **Rationale**: Simpler, more maintainable, nodes work in both runtime and static graphs
- **Current Situation**:
  - Node derive already generates event handling for runtime graphs (lines 622-635)
  - Uses `ProcessingContext` and calls `on_fieldname(&event, context)` handlers
  - Event outputs stored in IO struct as `Vec<EventInstance>` (line 224-226)

### 3.2 Handle_events generation ⏳ PREPARED (temporarily disabled)
- **File**: `oscen-macros/src/lib.rs` (lines 637-673)
- **Status**: ⏳ Code written but commented out until Phase 4
- **What was added**:
  - `handle_events(&mut self, ctx: &mut StaticContext)` method generation
  - Dispatches events from `{fieldname}_events` storage fields to `on_fieldname` handlers
  - Currently disabled because nodes don't have storage fields yet
- **Blockers**:
  - Nodes need manual addition of `pub {fieldname}_events: StaticEventQueue` fields
  - Event handlers need to work with `StaticContext` (currently take `ProcessingContext`)

### 3.3 Requirements for static-compatible nodes 📋
- **Manual changes needed** (to be done in Phase 4):
  1. Add event storage fields to node struct:
     ```rust
     pub gate_events: StaticEventQueue,  // for #[input(event)] gate
     pub trigger_events: StaticEventQueue,  // for #[output(event)] trigger
     ```
  2. Update event handler signatures to work with both contexts:
     - Option A: Make generic: `fn on_gate<Ctx>(&mut self, event: &EventInstance, ctx: &mut Ctx)`
     - Option B: Duplicate: `on_gate_static` for static graphs
     - Option C: Single handler, ignore context (most handlers don't use it anyway)
  3. Initialize storage fields in `new()`: `gate_events: StaticEventQueue::new()`

## Phase 4: Update Existing Nodes

### 4.1 Fix simple nodes (non-event-emitting)
- **Files**: `examples/electric-piano/src/tremolo.rs`, `examples/electric-piano/src/harmonic_envelope.rs`
- Change `new()` → `new(sample_rate: f32)`
- Make input/output fields `pub`
- ✅ Already done for Tremolo

### 4.2 Update OscillatorBank
- **File**: `examples/electric-piano/src/electric_piano_voice.rs`
- Add `#[node(static_compatible)]` attribute
- Change `new()` → `new(sample_rate: f32)`
- Make fields `pub`
- Update `on_gate` signature: `fn on_gate(&mut self, event: &EventInstance, _ctx: &mut StaticContext)`
- No event emission needed (gate just resets phase)

### 4.3 Update AmplitudeSource
- **File**: `examples/electric-piano/src/electric_piano_voice.rs`
- Add `#[node(static_compatible)]` attribute
- Change `new()` → `new(sample_rate: f32)`
- Make fields `pub`
- Update `on_gate` signature with `StaticContext`
- No event emission needed

### 4.4 Update event-emitting nodes
- **Files**: `oscen-lib/src/midi.rs`, `oscen-lib/src/voice_allocator.rs`
- **MidiParser**: Update `on_midi_in` to use `ctx.emit_event()` instead of `context.emit_timed_event()`
- **MidiVoiceHandler**: Update `on_note_on`/`on_note_off` to use StaticContext
- **VoiceAllocator**: Update event emission to use StaticContext
- Change all `new()` → `new(sample_rate: f32)` (accept unused parameter)
- Make input/output fields `pub`

## Phase 5: ElectricPianoVoiceNode Static Graph

### 5.1 Update graph definition
- **File**: `examples/electric-piano/src/electric_piano_voice.rs`
- Verify graph macro generates correct static struct with:
  - Event queue fields for `gate` input
  - Proper event routing in `process()`

### 5.2 Update main ElectricPianoGraph
- **File**: `examples/electric-piano/src/main.rs`
- Keep `compile_time: true`
- Update code to use static graph API:
  - Remove `.graph` field access → direct field access
  - Replace `graph.queue_event()` → `synth.midi_parser_midi_in_events.push(event)`
  - Replace parameter setters → direct field assignment: `synth.brightness = value`

### 5.3 Update audio callback
- Remove `graph.process()` → `synth.process()`
- Remove `get_value()` calls → direct field access: `synth.left_out`, `synth.right_out`

## Phase 6: Testing & Validation

### 6.1 Add unit tests
- **File**: `oscen-lib/src/graph/static_context.rs`
- Test event emission and queueing
- Test queue overflow behavior (panics or silently drops?)

### 6.2 Test static graph with events
- Create simple test case: one node with event input → event output
- Verify event routing works correctly
- Verify ArrayVec capacity limits are respected

### 6.3 Build and test electric-piano
- `cargo build --package electric-piano`
- Test MIDI input → sound output
- Verify no performance regression (should be faster!)

## Phase 7: Documentation

### 7.1 Update CLAUDE.md
- Document static graph event limitations (fixed capacity)
- Explain when to use static vs runtime graphs
- Add examples of static-compatible event handlers

### 7.2 Add inline documentation
- Document StaticContext API
- Add doc comments to static_compatible attribute
- Document ArrayVec capacity choices

## Success Criteria
- ✅ Electric-piano compiles with `compile_time: true`
- ✅ MIDI events flow through static graph correctly
- ✅ No heap allocations during `process()` (verify with profiler)
- ✅ Event queue overflows are handled gracefully (panic in debug, ignore in release?)
- ✅ Performance is measurably better than runtime graphs

## Progress Summary

**Completed:**
- ✅ Phase 1: Core infrastructure (StaticContext, StaticEventQueue, types)
- ✅ Phase 2.1-2.2: Static graph event field generation and initialization
- ✅ Phase 3: Macro approach decided (no attribute needed, handle_events prepared)

**In Progress:**
- ⏳ Phase 4: Update existing nodes (ready to start)

**Blocked:**
- 🔒 Phase 2.3-2.4: Event handling in process() (blocked on Phase 4 node updates)
- 🔒 Phase 5-7: Integration, testing (blocked on Phase 2.3-2.4)

**Key Files Modified:**
- `oscen-lib/src/graph/static_context.rs` (new)
- `oscen-lib/src/graph/types.rs` (added StaticEventQueue)
- `oscen-lib/src/graph/mod.rs` (exports)
- `oscen-macros/src/graph_macro/codegen.rs` (event field generation)
- `oscen-macros/src/lib.rs` (handle_events generation prepared)

**Compilation Status:** ✅ All changes compile successfully

## Estimated Effort
- Phase 1-2: Core infrastructure - 4-6 hours ✅ ~3 hours actual
- Phase 3: Macro updates - 3-4 hours ✅ ~1 hour actual (simpler than expected)
- Phase 4: Node updates - 3-4 hours ⏳ Ready to start
- Phase 5: Electric-piano integration - 2-3 hours
- Phase 6-7: Testing & docs - 2-3 hours
- **Total: ~14-20 hours** (4/14-20 complete, ~20% done)

## Risks & Mitigations
- **Risk**: ArrayVec capacity too small → events dropped
  - *Mitigation*: Make capacity configurable via attribute, add debug assertions
- **Risk**: Breaking existing runtime graph nodes
  - *Mitigation*: Make static_compatible opt-in, keep default behavior unchanged
- **Risk**: StaticContext adds unexpected overhead
  - *Mitigation*: Benchmark early, inline aggressively with `#[inline(always)]`

## Background Research Summary

### How Events Work in Runtime Graphs

**Event Flow:**
1. External code queues events via `graph.queue_event(input, frame_offset, payload)`
2. During `graph.process()`, events are routed to node input queues
3. Node's `read_inputs()` calls event handlers (`on_fieldname`) with ProcessingContext
4. Handlers can emit events via `context.emit_event(output_index, event)`
5. Emitted events are routed to connected node inputs
6. External code can drain output events via `graph.drain_events(output, handler)`

**Key Components:**
- `EventInstance`: Timestamped event with frame_offset and EventPayload
- `EventPayload`: Either Scalar(f32) or Object(Arc<dyn EventObject>)
- `EventQueue`: Vec-based queue per endpoint (max 256 events)
- `ProcessingContext`: Provides event slices and emission methods
- Node macro generates `on_*` handlers for each `#[input(event)]` field

### What Static Graphs Currently Lack

1. **No ProcessingContext** - Static graphs bypass it entirely
2. **No event queues** - Events managed by Graph's endpoint storage
3. **No event handler dispatch** - Callbacks require ProcessingContext
4. **No pending_events Vec** - Event routing done by Graph
5. **No endpoint handles** - Static graphs use direct field access

Event inputs currently try to use temporary graph for EventParam creation, which won't work for fully static graphs.

## Next Steps

### Immediate: Phase 3 - Node Macro Updates

The critical blocker is updating the `#[derive(Node)]` macro to support static graphs with events. Here's the approach:

**Option 1: Opt-in Static Compatibility (Recommended)**
- Add `#[node(static_compatible)]` attribute
- Generate additional code for static graphs:
  - Event storage fields on the node struct itself (not IO struct)
  - `handle_events(&mut self, ctx: &mut StaticContext)` method
  - Keep existing runtime graph code unchanged
- Pros: Backward compatible, explicit opt-in
- Cons: More complex macro, dual code paths

**Option 2: Always Generate Both**
- Remove the opt-in, always generate static support
- Simpler macro logic
- Pros: All nodes work with both graph types
- Cons: May add overhead to nodes that don't need static support

**Recommendation**: Start with Option 1 for safety, can migrate to Option 2 later if desired.

### Implementation Details for Phase 3.1:

1. **Parse `#[node(static_compatible)]` attribute**
   - Check for attribute in `derive_node()` function
   - Set a flag to enable static code generation

2. **Add event storage fields to node struct**
   - For each `#[input(event)]` field, generate:
     ```rust
     pub fieldname_events: StaticEventQueue
     ```
   - For each `#[output(event)]` field, generate:
     ```rust
     pub fieldname_events: StaticEventQueue
     ```
   - These are in addition to the endpoint handles

3. **Generate `handle_events()` method**
   ```rust
   impl NodeName {
       pub fn handle_events(&mut self, ctx: &mut StaticContext) {
           // For each event input field
           for event in &self.fieldname_events {
               self.on_fieldname(event, ctx);
           }
       }
   }
   ```

4. **Update event handler signatures**
   - Handlers should accept both `&mut ProcessingContext` and `&mut StaticContext`
   - Or use a trait to abstract over both
   - Or require two implementations (runtime vs static)

**Questions to Resolve:**
- Should event handlers take `StaticContext` or `ProcessingContext`?
  - Probably need both: `on_fieldname_static(&mut self, event: &EventInstance, ctx: &mut StaticContext)`
  - Or make `on_fieldname` generic over context type?
- Where should event storage fields be initialized?
  - In the `new()` constructor with `StaticEventQueue::new()`
- Should nodes without `static_compatible` work in static graphs?
  - No - static graphs should require all nodes to be static-compatible
