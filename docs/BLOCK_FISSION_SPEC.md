# Spec: Block-Level Codegen via Loop Fission

Status: draft, not yet scheduled.
Prerequisite reading: `oscen-graph-compiler/src/ir/lower.rs` (`topo_sort`),
`oscen-graph-compiler/src/codegen/emit_frame.rs` (multirate scheduler — the
per-frame miniature of this design).

## 1. Motivation

`process_block(frames)` currently loops `__advance_one_frame` →
`__frame_core`, where `__frame_core` is the entire graph in topological
order. Every node's state is touched every sample inside one giant loop
body with cross-iteration dependencies through every stateful node, so:

- LLVM cannot autovectorize anything: the mega-loop is serial by
  construction, even for long feedforward chains of trivially
  vectorizable nodes (gains, waveshapers, mixers).
- The per-sample working set is the *whole graph's* state. Large graphs
  drag every node's fields through cache 512 times per block.
- Node arrays (`Voice[8]`) are processed voice-at-a-time inside the frame
  loop, never as the data-parallel batch they actually are.

Loop fission inverts the nesting: instead of *for each frame, for each
node*, emit *for each region, for each frame*, with block buffers between
regions. Small per-region loops vectorize; each region's state stays hot
across the whole block.

**Goals**, in priority order:

1. Bit-exact output. This transformation must not change the sound. The
   golden render suite (`oscen-lib/tests/golden_render.rs`) is the
   acceptance gate, not a nice-to-have.
2. No latency change. Feedback loop delay stays exactly the `N` the user
   wrote, independent of block size.
3. Measurable throughput wins on the `synth_app` and `graph_blocks`
   benches for feedforward-heavy and voice-array graphs.
4. No regression on feedback-heavy graphs (fallback heuristic, §8).

**Non-goals** (§11): fast-math, per-node SIMD rewrites, changing any
node's arithmetic, block-rate parameter semantics.

## 2. Terminology

- **Region**: a maximal set of nodes emitted as one `for frame in
  0..frames` loop. Either *feedforward* (no intra-sample dependency
  cycles) or *recurrent* (contains at least one feedback cycle).
- **Interior edge**: an edge between two nodes in the same region.
  Emitted exactly as today (field-to-field `connect` inside the loop).
- **Boundary edge**: an edge between regions. Materialized as a block
  buffer: the source region writes `frames` values, the destination
  region reads them.
- **Chunk**: for recurrent regions closed by a literal delay
  `-> [N] ->`, a sub-block of ≤ N frames that can be processed before the
  recurrence needs a value from it (§7).

## 3. Region partitioning

Runs as a new IR pass (`ir/passes/regions.rs`) after `topo_sort` and
`dead_nodes`:

1. Build the dependency graph over `ir.processors` using **all** edges,
   *including* `is_feedback` edges (unlike `topo_sort`, which skips them —
   that's what makes the per-frame schedule acyclic; here we want the
   cycles visible).
2. Compute strongly-connected components (Tarjan). Every feedback cycle —
   the via `Delay` plus the entire loop path — lands in one SCC.
   Singleton SCCs with no self-loop are feedforward.
3. Condense to a DAG and topologically order it, tie-broken by source
   order (same determinism rule as `topo_sort`).
4. **Merge adjacent feedforward SCCs** into larger regions when doing so
   creates no cycle in the condensation. Merging is a codegen-quality
   knob: maximal merging reproduces today's single loop; no merging gives
   one region per node. Initial policy: merge greedily along the topo
   order, but *never* merge a recurrent SCC with anything, and split at
   rate boundaries (§9) and node-array boundaries (§10).
5. Classify every edge as interior or boundary; store the region list
   (member `NodeId`s in topo order, region kind, closing-delay info for
   recurrent regions) on `IrGraph`.

The pass is analysis-only; it must not change `edge_order` (per-edge
resampler/buffer field naming depends on it).

## 4. Codegen changes

### 4.1 Struct fields

Each boundary **stream** edge gets a block buffer field, following the
existing graph-I/O pattern:

```rust
__edge7_block: [f32; DEFAULT_MAX_BLOCK_SIZE],
```

Naming reuses the `edge_order` index (same convention as
`resampler_field_name`). `Frame<N>` payloads use the payload type as the
element. Boundary **value** edges whose source can change per frame
(stream-derived expressions, ramped inputs) buffer identically; constant-
within-block value edges are latched once at region start (this is the
same TYPED-value latch rule `emit_frame.rs` already applies across the
multirate inner loop). Boundary **event** edges: §6.

Memory cost: 4 bytes × `MAX_BLOCK_SIZE` per boundary f32 edge (2 KiB at
the default 512). The merge policy (§3.4) directly controls this; the
struct-size delta must be reported in the snapshot diff review.

### 4.2 `process_block` body

```rust
pub fn process_block(&mut self, frames: usize) {
    debug_assert!(frames <= Self::MAX_BLOCK_SIZE);
    self.__materialize_ramps(frames);          // §5
    // Region 0 (feedforward): per-region frame loop
    for __frame in 0..frames {
        // interior connects + process calls for region-0 nodes,
        // boundary reads from input/__edgeK_block[__frame],
        // boundary writes to __edgeJ_block[__frame]
    }
    // Region 1 (recurrent): identical shape, still sample-serial inside
    for __frame in 0..frames { ... }
    ...
}
```

Each region loop body is the existing `__frame_core` emission machinery
(`generate_process_body`, `emit_same_rate_processes`, the connect/process
interleave) filtered to the region's members — the emitters already take
node lists, so this is a scoping change, not a rewrite.

### 4.3 Single-frame path

`process()` / `__frame_core` remain exactly as today (monolithic,
unfissioned). Both paths are bit-exact (§6), so a graph driven per-sample
and per-block stays in lockstep; the realtime-safety and golden tests
cover both. Graphs where the partition degenerates (§8) emit only the
monolithic path, i.e. zero diff versus current output.

## 5. Ramps

`tick_ramps()` advances every ramped value input once per frame at graph
scope. Under fission, region 2's frame 100 executes long after region 1's
frame 100, so ramps can no longer be ticked inline. Instead
`__materialize_ramps(frames)` runs the existing per-frame ramp update
loop once, writing each ramped input's per-frame values into a block
buffer (`__ramp_<name>_block`); region loops read
`__ramp_<name>_block[__frame]` where they previously read the field.
Same values in the same order — bit-exact by construction. Non-ramped
value inputs are unaffected (plain field reads).

## 6. Bit-exactness argument and event handling

Each node is a state machine: output sequence = f(initial state, input
sequence). Fission reorders computation *across* nodes but preserves
every node's input sequence exactly — node B still consumes A's outputs
for frames 0..n in order; fan-in summation order per edge is unchanged;
connection expression trees are unchanged; stateful resamplers see
identical inputs so their state evolves identically. rustc/LLVM do not
reassociate IEEE float ops without fast-math, and vectorized IEEE add/mul
are per-lane exact. Therefore the transformation is bit-exact, and
`golden_render.rs` verifies it directly.

**Events.** Today an event emitted by node A at frame `t` is visible to
downstream B *in the same frame* because A precedes B in topo order.
Fission preserves this: boundary event edges buffer the source's emitted
`EventInstance`s across the whole block (offsets are already carried by
`frame_offset`; capacity is the existing `StaticEventQueue` — since the
source's per-block emission may exceed the per-frame queue size, boundary
event buffers are sized `MAX_STATIC_EVENTS_PER_ENDPOINT` and overflow
follows the existing debug-panic/release-drop policy, with the limit
called out in the generated doc comment). The destination region drains
events whose `frame_offset == __frame` at the top of its loop iteration —
the same replay-at-offset model the cross-rate `Multiply`/`Divide` drains
in `emit_frame.rs` already implement. Interior event edges are unchanged.

Note the constraint this imposes on partitioning: a *value or stream*
side effect of an event handler is only bit-exact if the handler runs
before the frames that observe it — which the offset-replay rule
guarantees, because the destination processes frame `t` events before its
frame-`t` `process()` call, exactly as today.

## 7. Recurrent regions: serial by default, chunked when licensed

A sample recurrence is irreducibly serial; recurrent regions keep today's
code shape. Two refinements:

- **Chunked execution for literal delays.** A cycle closed by
  `-> [N] ->` (`DelayVia::Samples`, N known at compile time) has the
  property that output at frame `t` depends on loop input at `t − N`.
  Processing the region in chunks of `C = min(N, frames_remaining)`
  frames never reads a value that hasn't been computed. Emission: an
  outer chunk loop, inner per-frame loop of length ≤ C. This turns
  long-delay feedback (comb filters, feedback delays, Karplus–Strong)
  into mostly-block code. **Hard invariant: chunk ≤ N**, enforced by the
  region pass (the chunk size is a compile-time constant derived from the
  minimum literal delay on any cycle through the SCC).
- **Named vias stay serial.** `-> [name] ->` only guarantees
  `AllowsFeedback`; the delay amount is unknown, so C = 1 (i.e. plain
  serial). Follow-up (out of scope here): add
  `const MIN_LATENCY_SAMPLES: usize` to `AllowsFeedback` and thread it
  through `IrNode.latency_samples` to license chunking for named vias.

No latency is introduced anywhere: the recurrence is computed sample-
exactly inside the block. (Contrast with runtime node graphs that insert
an implicit one-*block* delay on feedback wires and change sound with
buffer size — explicitly rejected here.)

## 8. Degeneration fallback

Feedback-heavy graphs collapse to one dominant recurrent region; fission
then buys nothing and boundary buffers add pure overhead (field-to-field
register handoff becomes a 512-float round trip through memory).

Policy: after partitioning, if the largest region contains ≥ P% of
processor nodes (initial P = 80) or the partition yields fewer than 2
feedforward regions with ≥ 2 nodes each, **emit the current monolithic
`process_block` unchanged**. The threshold is a constant in the region
pass, to be calibrated against `graph_blocks`/`synth_app` criterion
baselines rather than guessed. Snapshot tests pin both shapes (a graph
above and below threshold).

## 9. Multirate interaction

Fission applies at the outer rate. The multirate inner loop
(`generate_multirate_inner_body`) is already a self-contained schedule
with its own staging buffers; treat each *rate group* as a region-
boundary constraint in phase 1 (never merge across `NodeRate` changes —
the existing pre-inner/inner/post-inner bucketing becomes three-plus
regions naturally). Fissioning *inside* the ×N inner loop is deferred; the
per-edge `up_buf`/`down_buf` staging generalizes but the win is smaller
(inner loops are short) and the emission complexity is real.

## 10. Voice-array regions (cross-voice SIMD)

`FanoutShape::Parallel { n }` already models N independent identical
state machines. A `NodeArray` whose elements have no inter-element edges
becomes its own region emitted as:

```rust
for __frame in 0..frames {
    for __k in 0..N { /* per-voice connects + process */ }
}
```

with `__k` innermost — unit-stride over `[Voice; N]`, independent
iterations, which is the shape LLVM's loop vectorizer wants. This is
likely the bigger SIMD win than time-direction vectorization for the
poly-synth workload (the `synth_app` bench is the arbiter). SoA layout
for voice state is a possible follow-up, not part of this spec.

Interaction with future voice gating (`is_active`): a per-voice-array
region is also the natural granularity for skipping idle voices; the
region structure is a prerequisite that design can build on.

## 11. Non-goals

- Any change to node-internal arithmetic, including fast-math flags or
  reassociation. Nodes with internal recurrences (IIR filters,
  envelopes) remain serial math inside their region loop; their benefit
  is locality only.
- Block-rate ("control-rate") parameter semantics. Ramps stay per-frame.
- Fissioning the single-frame `process()` path.
- Runtime (`DynGraph`) scheduling — this is static-graph codegen only.

## 12. Implementation phases

Each phase lands green against golden renders + realtime-safety tests
and re-blesses snapshots (`OSCEN_UPDATE_SNAPSHOTS=1`, diff reviewed).

1. **Region pass, analysis only.** `ir/passes/regions.rs`: SCC
   condensation, merge policy, degeneration heuristic, region metadata on
   `IrGraph`. Unit tests over hand-built IR (the `ir/graph.rs` test
   helpers already exist). No codegen change; zero snapshot diff.
2. **Feedforward fission, same-rate graphs, streams + plain values
   only.** Boundary buffers, per-region loops, ramp materialization,
   monolithic fallback. Graphs with boundary *event* edges force-merge
   (treat the event edge as a region-merge constraint) so phase 2 never
   has to buffer events. Snapshot + golden + `graph_blocks` baseline
   comparison.
3. **Boundary event buffering** (§6), lifting the phase-2 merge
   constraint. New golden test with an event-driven cross-region graph
   (sequencer → voice shape).
4. **Voice-array regions** (§10). `synth_app` baseline comparison is the
   success metric.
5. **Chunked recurrent regions for literal delays** (§7). New golden
   test: long-delay feedback (Karplus–Strong shape) verifying bit-exact
   equality with the serial emission.
6. Calibrate the degeneration threshold (§8) on benches; document the
   measured crossover in this file.

## 13. Verification

- **Bit-exactness**: every phase gated on `golden_render.rs` unchanged
  (no re-recording — if a golden changes, the phase has a bug). Add one
  golden per new structural shape (cross-region events, chunked
  feedback, voice-array region).
- **Equivalence property test**: drive the same graph per-sample
  (`process()`) and per-block (`process_block`) over identical inputs,
  assert bit-equality — pins the two-path invariant of §4.3.
- **Realtime safety**: region loops and buffers are preallocated struct
  fields; `realtime_safety.rs` gets a fissioned-graph case.
- **Performance**: criterion baselines before each phase
  (`cargo bench -p oscen --bench graph_blocks -- --baseline <name>`,
  same for `synth_app`); benches only trustworthy with nothing else
  compiling on the machine.
- **Snapshots**: at least one fissioned and one fallback (monolithic)
  graph pinned in `oscen-graph-compiler/tests/snapshots/`.

## 14. Open questions

1. Merge policy tuning (§3.4): maximal regions minimize buffer memory
   but recreate the mega-loop; per-node regions maximize vectorization
   but bloat buffers and code size. Likely answer: merge until a region
   contains a node with internal recurrence, then cut — needs
   measurement.
2. Should boundary buffers share storage when live ranges don't overlap
   (classic buffer coloring)? Deferred until struct sizes are measured;
   the multirate buffers don't bother either.
3. `AllowsFeedback::MIN_LATENCY_SAMPLES` (§7) — worth doing in the same
   arc, or separate design doc alongside `allows-feedback-refactor.md`?
4. Does `#[inline(always)]` on per-region fns fight or help LLVM's
   vectorizer at realistic graph sizes? Emit regions inline in
   `process_block` first; split into fns only if compile time or icache
   pressure says otherwise.
