# Architecture review: performance, abstractions, and DSL UX

Reviewed September 13, 2026, against `60588d4333f52cdc7fbf0d08eaa5e1b2090d194d`.
Source references below refer to that revision. This is a review, not an
implementation plan already approved or a claim that the findings are fixed.

## Status (2026-09-13, after the review)

Landed in the commits following `3a8e8d4`: C1 and C2 (a resolved
per-destination driver plan and a whole-expression clock in the IR, with
codegen emitting from the plan), C3 (trait `process` delegates), C4
(`<name>_block` event-output accumulators), C5 (idle handoff polls are one
atomic load; the swap cost is documented; plugin callbacks own their DSP and
use inline MIDI), C6 (descriptor probes run on a sized worker), and the ramp
half of C7 (`active_ramps` removed). Still open from C7: external asset
type/endpoint validation, the unconditional `BlockRender` `compile_error!`,
and asset loaders' rate under oversampling. Graph-side `EndpointAt`
markers for nested-graph destinations, the voice lifecycle (P1), the
storage split (P2), and block scheduling experiments (P3) are not started.
`docs/review-probes/` has been trimmed to the cases that still compile.

### Progress (2026-09-26)

Items 1 and 2 below have landed (`be703d2`, `e5d4ab0`, `42746fc`,
`7074261`); item 3 is next and still needs its design pass. Deviations from
the plan as written:

- *External validation* checks the bound endpoint by field type
  (`AssetInput<Playable>`, implemented only by `AssetSlot`) rather than
  requiring the literal name `asset`, which would have rejected the existing
  `reverb.ir` / `player.buf` bindings. The external's type is checked against
  a new `ExternalAsset` trait (only `AudioAsset`), since every consumer builds
  from an `AudioAsset` and the playable type is never user-facing. The
  declared type must now resolve, so `AudioAsset` has to be in scope.
- *Mixed-frame graphs* also lose `get_stream_output` when their stream
  outputs themselves mix frame types (it used to fall back to `f32` and
  mis-type `Frame<N>` outputs); otherwise it returns the outputs' shared
  frame type.
- *Graph-side markers*: the fan-in assertion does not name the free marker
  struct (`Gain__input__Ep` is not re-exported by the prelude, so a bare
  `Gain` destination would stop resolving). Instead the derive and `graph!`
  both emit a hidden inherent fn `<ep>__ep() -> PhantomData<marker>`, and
  the assertion infers the marker from it. This is stable Rust, so graph-only
  crates no longer need `inherent_associated_types` for fan-in checks
  (derived nodes still do, for their aliases). No change to
  `passes::drivers` was needed: its "no marker" rule only covers untyped
  constructors. The cross-rate `::State` projection still uses the
  `__Ep` aliases.

### Next steps (planned 2026-09-26)

Ordered by value and by how much each unblocks. Each item is one commit with
its own regression test, following the pattern of the fixes above. The
standard verification after every commit is unchanged: `cargo test -p oscen`
(no skips), `golden_render`, `realtime_safety`, `oscen-graph-compiler`,
`oscen-macros`; golden hashes are expected to stay unchanged throughout.

**1. Finish C7 (milestone 1). Three small, independent commits.**

- *Validate external asset bindings at macro time.* `resolve_asset_bindings`
  (`ir/lower.rs`) only checks that the destination node has a type path; the
  external's declared type and the named endpoint are never consulted, so
  `external impulse: DoesNotExist; impulse -> reverb.typo;` compiles and
  installs the convolver's asset consumer anyway. Fix: in codegen
  (`emit_struct.rs::generate_asset_wiring`) project the declared external type
  against `<Node as AssetEndpoint>::Consumer`'s playable type with a
  `const _: fn() = ..` assertion spanned at the `external` declaration, and
  require the bound endpoint name to be the node's single asset input (from the
  endpoint manifest when known; otherwise the literal `asset`). Tests: two
  trybuild fixtures (`asset_wrong_type.rs`, `asset_wrong_endpoint.rs`) plus a
  positive `oscen-lib` test that the correct spelling still installs. The
  `WrongAsset` case in `docs/review-probes/src/bin/dsl.rs` becomes a negative
  fixture and leaves the probe.
- *Omit, don't reject, the offline `BlockRender` impl for mixed-frame graphs.*
  `generate_block_render_impl` (`codegen/mod.rs`) returns a `compile_error!`
  for `BlockFrameTy::Mixed` even though the realtime interface is fine. Fix:
  return an empty token stream and record the reason as a doc comment on the
  struct; a graph that genuinely needs offline rendering gets a normal missing-
  trait error at the call site. Tests: a compiler regression test that the
  emitted tokens contain neither `compile_error` nor `impl BlockRender` for a
  mixed graph, an `oscen-lib` test that such a graph builds and processes, and
  the existing snapshots must not change.
- *Give asset loaders the node's effective rate.* `generate_asset_set_graph_rate_calls`
  (`emit_struct.rs`) passes the base `sample_rate` to every `AssetLoadHandle`,
  while `generate_node_set_sample_rate_calls` scales the consuming node's rate
  by its `* N` annotation. A sample or impulse response prepared for an
  oversampled convolver is therefore conformed to the wrong timebase. Fix:
  use `scaled_rate_expr(node.rate)` for the bound node in the asset call, and
  document on `AssetLoadHandle::set_graph_rate` that the rate is the consumer's.
  Tests: a compiler regression test that the emitted call carries the scaled
  expression, and an `oscen-lib` test that an `external` bound to a `* 2` node
  reports `graph_rate == 2 * sample_rate` (via a `SampleRateMismatch` error on
  a base-rate asset) and accepts a doubled-rate one.

**2. Graph-side `EndpointAt` markers (plan step B5, milestone 2).**

The driver plan's rustc fan-in veto (`generate_fan_in_assertions`) projects
`<Path>::<field>__Ep`, an inherent associated type that only `#[derive(Node)]`
emits. Nested graphs emit none, so an unknown-kind multi-driver into a nested
graph's endpoint currently surfaces as a raw "associated type not found"
error rather than the `FanInAllowed` diagnostic.

Doing it the derive's way would put `pub type <field>__Ep = ..;` inside the
graph's inherent impl, and *defining* an inherent associated type requires
`#![feature(inherent_associated_types)]` in every crate that uses `graph!`.
Avoid that:

- Change `endpoint_marker_tokens_for` (`codegen/mod.rs`) to name the free
  marker struct instead of the alias: for a type path `a::b::Node` the marker
  is `a::b::Node__<field>__Ep`, which the derive already emits as a sibling
  `pub struct`. This drops the inherent-type dependency from the assertion for
  derived nodes too; the aliases stay for user code.
- Add `generate_endpoint_markers` to `codegen/mod.rs`, walked the same way as
  `generate_endpoint_manifest`: one `pub struct <Graph>__<ep>__Ep;` and one
  `impl EndpointAt<..> for <Graph> { type Kind; type Frame }` per graph
  endpoint, with `Kind`/`Frame` from the declared endpoint type. No inherent
  aliases. Register the names in `validate_names.rs`.
- Drop the "no marker on a multi-driver destination → macro-time diagnostic"
  rule in `passes::drivers` for nodes whose type path is a graph; keep it for
  untyped constructors.
- Tests: `oscen-lib` test that a value endpoint of a nested graph rejects two
  drivers with the `FanInAllowed` E0277 (trybuild fixture
  `nested_graph_value_fanin.rs`), and that a stream endpoint of a nested graph
  sums two drivers; compiler regression test for the marker emission. Re-bless
  all five snapshots and review the diff, which should be additive only.
- Risk: marker names could collide with a user type in the same module; use
  the same `__Ep` suffix the derive uses so the collision rules are shared.

**3. Voice lifecycle (P1, milestone 4).**

The measured whole-synth cost is dominated by configured-but-idle voices.
This is a design job, not a fix: it needs an explicit sleep/wake/tail state on
`VoiceAllocator` and the voice handler, a rule for when a sleeping voice's
downstream reads are valid, and the `synth_app` bench extended with an
idle-heavy fixture before any code changes. Plan it in its own pass once
items 1 and 2 have landed; P2 (storage split) and P3 (block scheduling) stay
behind it.

Smaller items to fold into whichever of the above touches the code first:
`raw_midi_event` allocates for messages shorter than three bytes; the
standalone `fm-synth` binary still frees a `Vec<u8>` MIDI message on the audio
thread; the Object-event retirement protocol is undocumented.

## Executive assessment

**Keep the static compiler and field-based DSP model. Make their contracts
consistent before expanding the DSL or rebuilding the block scheduler.**

Oscen's strongest idea is that a node is an ordinary Rust struct, and a graph
compiles into direct operations on those structs. The separation of the compiler
from the proc-macro crate, typed dispatch, explicit feedback delays, frame types,
parameter reflection, and shared sample/block processing body are good foundations.
This does not need a dynamic graph rewrite or a large general-purpose compiler
framework.

The main architectural weakness is that the implementation has several different
answers to the same semantic question. Endpoint kinds can come from declarations,
connection inference, manifests, or Rust traits. Addressing and fan-in are partly
resolved in codegen. Graphs have a working inherent processing method but an empty
trait implementation. Metadata queries construct DSP objects. The audio/control
ownership boundary is an intention rather than a consistently enforced API.

Those are not merely aesthetic concerns: small probes reproduce dropped events,
writes to the wrong voices, operand-order-dependent resampling, a no-op processor
trait, and audio-thread allocation/destruction. They are the first priorities.

For performance, the clearest measured opportunity is **configured-but-idle voice
work**. In the whole-synth benchmark, eight idle voices cost approximately 85% of
eight sounding voices; configuring sixteen voices approximately doubles the cost
of the same eight-note chord. Address that with an explicit voice lifecycle, not
an unsafe blanket `if !is_active() { skip }` optimization.

### Recommended order

1. Repair silent DSP/API errors and real-time ownership violations.
2. Introduce a small, authoritative analyzed graph plan; unify endpoint, driver,
   address, and expression-clock decisions there.
3. Separate lightweight configuration/metadata, audio state, and host/control
   adapters. Make the processor trait truthful and protect ramp invariants.
4. Implement explicit voice sleep/wake/tail semantics and measure state/storage
   improvements.
5. Experiment with selective block/span execution only after semantic and
   benchmark gates are solid.

## Scope and evidence

Reviewed the three core crates, DSP/runtime and asset paths, compiler passes and
codegen, node derive, examples, tests, benchmarks, and design notes. Three parallel
static reviews covered runtime performance, DSL/compiler UX, and public APIs/host
integration. Findings were then checked against source and targeted executable
probes. No production implementation was changed.

Labels used below:

- **Reproduced:** executed against the reviewed revision.
- **Source-confirmed:** the relevant implementation is explicit; the particular
  end-to-end scenario was not executed.
- **Hypothesis/proposal:** requires profiling, experiments, or a design decision.

### Validation results

| Check | Result |
|---|---|
| `cargo test -p oscen -p oscen-graph-compiler -p oscen-macros` | Aborts in the existing small-stack metadata test |
| `cargo test -p oscen -- --skip param_descriptors_survive_small_stack_thread` | 407 passed, 1 ignored, 1 filtered out |
| `cargo test -p oscen-graph-compiler -p oscen-macros` | 199 passed, 7 ignored |
| `cargo check -p oscen --no-default-features` | Passed |
| `cargo check -p oscen --no-default-features --features fft` | Passed |
| `cargo test -p oscen-macros --features nih-plug --test nih_params_test` | Fails to compile: missing comma before `center` at `nih_params_test.rs:15` |
| Manual API, DSL, and real-time probes | Reproduced the discrepancies listed below |
| `per_sample`, `graph_blocks`, `synth_app` Criterion benches | Ran serially; caveats below |

The skipped-test run is **not** a green full-suite result. No plugin bundling,
DAW interaction, clean-build scaling study, assembly audit, hardware-counter
profile, or worst-case callback-latency study was performed.

## 1. Existing architecture: what to preserve

### Compiler boundary

`oscen-graph-compiler/src/lib.rs:27–84` exposes compiler and manifest-aware entry
points without requiring a proc-macro invocation. Parsing, lowering, dead-node
removal, and emission are distinct stages. SlotMap node/edge identities,
canonical edge order, and adjacency validation are useful foundations for further
analysis (`src/ir/graph.rs:88–116`, `src/ir/validate.rs`).

The weakness is not the crate boundary. It is that the IR is not yet sufficiently
resolved for codegen to be mostly mechanical.

### Runtime and node authoring

Plain fields plus `SignalProcessor::process` are much easier to understand than a
runtime endpoint registry or mandatory processing context. `ConnectEndpoints`
and `EndpointAt` let Rust prove payload compatibility and specialize direct
operations. Preserve that optimizer visibility and the ordinary-Rust escape hatch.

`AudioFrame` and `Frame<N>` provide a useful basis for channel-generic DSP.
`ValuePayload: Copy` is a sensible ownership restriction for small control values.
Keep a distinction between an automatable/ramped scalar parameter, a stream, and
a typed configuration value; do not make all of them one ambiguous signal type.

### Useful existing invariants

- Shared `__frame_core` reduces sample/block behavior drift
  (`codegen/emit_frame.rs:21–49`).
- Explicit delay brackets and atomic subgraph scheduling give feedback a clear
  sample-level meaning. Preserve them; transparent subgraph flattening is not a
  prerequisite for a better DSL.
- Bounded queues, event-empty fast paths, preallocated FFT scratch, cached ADSR
  and filter coefficients, and off-thread asset preparation are good choices.
- Golden renders complement sample/block equivalence, compile-fail tests, and
  token snapshots. The project has substantial coverage; the important missing
  layer is feature-interaction and lifecycle testing.

## 2. Urgent correctness and contract findings

### C1. Endpoint knowledge changes fan-in semantics

**Reproduced. High priority: silently changes audio/event delivery.**

Two graph event inputs connected to the same node event input deliver only the
last source:

```rust
input a: event;
input b: event;
connections {
    a -> sink.ev;
    b -> sink.ev;
}
```

The probe pushes one event to each input and observes `a = 0, b = 1`, not `1, 1`.
Known events bypass the accumulation path in
`oscen-graph-compiler/src/codegen/emit_node.rs:35–84`; each connection clears its
destination (`oscen-lib/src/graph/static_context.rs:195–218`). The existing
`oscen-lib/tests/event_fanin.rs:88–125` covers unanchored node-to-node sources,
which take a different path and do accumulate.

Conversely, two unanchored node **value** outputs connected to a node value input
compile and sum: the probe obtains `3` from `1` and `2`, although known value
fan-in is rejected. Codegen explicitly treats unknown kinds as summable
(`emit_node.rs:36–49`). This is an acknowledged implementation tradeoff, but a
bad public contract: adding a graph-level anchor should not determine whether a
value connection is legal.

**Recommendation:** resolve a driver group once per destination. A stream group
sums in a defined order; an event group clears once and appends in a defined
order; a value group rejects multiple drivers. Enforce the endpoint's declared
contract, not merely what inference happened to discover. Rust-side marker
assertions/dispatch can cover cases unavailable to the proc macro; unknown must
not mean "probably stream."

**Gate:** anchored/unanchored, scalar/indexed, graph/node-output, and cross-rate
fan-in tests exercising actual rendered behavior, not just successful token output.

### C2. Addressing and expression clocks are not compositional

**Reproduced. High priority: accepted syntax performs the wrong operation.**

```rust
gain * 0.5 -> voices[2].input;
```

With four zero-initialized gains and `gain = 2`, all four inputs become `1`.
Expected: only index 2 changes. The compound-source branch notices the destination
is an array but ignores its selected index
(`oscen-graph-compiler/src/codegen/emit_node.rs:229–260`). The multi-source sum
branch likewise constructs an unindexed target at `:210–215`.

A second probe uses a base-rate counter and a counter at `*4`:

```rust
slow.output + fast.output -> sink.input;
// versus
fast.output + slow.output -> sink.input;
```

Both compile. The first starts `[1, 6, 11, 16, ...]`; the second starts near zero
with a sinc startup transient. This is not floating-point reassociation: changing
the first operand changes the edge's rate classification and resampling.
`analyze_rates` uses the first referenced node's rate
(`oscen-graph-compiler/src/ir/lower.rs:1071–1092`,
`src/ir/expr/mod.rs:114–129`), not a resolved clock for the whole expression.

**Recommendation:** represent node selection and channel selection separately,
normalize all destinations through one addressing abstraction, and determine an
expression's clock from all dependencies. Initially reject ambiguous combinations
of independently clocked sampled dependencies rather than inventing an implicit
clock from operand order; define how constants and held/ramped values participate
and suggest explicit conversions. Group or reject overlapping broadcast/indexed
drivers explicitly.

**Gate:** tests where adding an identity expression preserves the addressed slot;
operand permutation does not change the clock policy; distinct untouched voice
indices remain untouched; unsupported combinations return diagnostics.

### C3. The advertised processor trait does not process generated graphs

**Reproduced. High priority; small immediate fix before broader API redesign.**

```rust
fn tick<T: SignalProcessor>(processor: &mut T) {
    processor.process();
}
```

For a generated gain graph with input `3` and gain `2`, this leaves output `0`.
Calling the graph's inherent `.process()` produces `6`.
`oscen-graph-compiler/src/codegen/mod.rs:1586–1599` emits an empty trait method;
the working inherent method is at `:638–647`. Nested graphs hide the problem by
calling inherent methods on concrete types.

**Recommendation:** delegate the trait method to the actual processing body.
Then define a small lifecycle contract that includes the operations graph
scheduling really uses: sample-rate distribution, preparation, event dispatch,
and sample processing. Today some are only inherent-method conventions.
Keep derive responsible for boilerplate, not node authors.

**Gate:** inherent, generic, UFCS, and trait-object entry points produce equivalent
results. Also test aliases of `SampleRate`: derive currently recognizes its
textual final identifier, not an equivalent Rust type
(`oscen-macros/src/lib.rs:66–67,246–252`).

### C4. The block API cannot retain earlier graph-output events

**Reproduced. High priority for host MIDI/event-output support.**

A graph directly forwarding event input `e` to event output `o` receives an event
at offset 1 and processes four frames. Afterwards `o.len() == 0`.
Stream outputs are staged across a block, but event outputs are overwritten on
each frame (`oscen-graph-compiler/src/codegen/emit_frame.rs:56–90`;
`oscen-lib/src/graph/static_context.rs:208–218`). This is a missing block-output
contract even where internal, same-frame event delivery works.

**Recommendation:** distinguish instantaneous internal event queues from bounded,
block-relative host output queues. Clear the latter once per block. Define internal
timestamp domains and normalize/stamp collected events into host-block-relative
offsets without double-offsetting forwarded events. Specify overflow behavior;
per-frame capacity and per-block capacity are different requirements. Include
nodes generating events at nonzero frames and nested/cross-rate output cases in
the regression tests, not just passthrough.

Related source-confirmed concern: input dispatch is grouped by field declaration
order (`oscen-macros/src/lib.rs:320–349`), and block staging sorts timestamp ties
without an explicit stable ordering contract (`codegen/mod.rs:920`). Splitting
note-on/off into separate endpoints can reorder same-frame retriggers. A single
ordered voice-command stream is a better long-term abstraction than trying to
reconstruct an ordering after splitting. This retrigger scenario was not executed.

### C5. The real-time boundary does not cover cold use or destruction

**Reproduced in release mode; source-confirmed in example callbacks. High priority.**

The runtime probe reports:

```text
fresh audio thread, first handoff take+retire: 1 allocation, 0 deallocations
same thread, subsequent empty take:           0 allocations, 0 deallocations
preallocated Object event, process_block(2):  0 allocations, 1 deallocation
```

`Consumer::take` is described as one allocation-free atomic operation but calls
`ArcSwapOption::swap(None)` (`oscen-lib/src/handoff/mod.rs:68–75`). In the resolved
**arc-swap 1.9.1** source, `src/lib.rs:485–495` calls `wait_for_readers`,
`src/strategy/hybrid.rs:199–205` calls `Debt::pay_all`, and
`src/debt/list.rs:151–170,223–229` can allocate a thread bookkeeping node on first
use. The warmed path still performs debt bookkeeping; it is not just one pointer
exchange. SamplePlayer and Convolver poll from their per-sample paths
(`oscen-lib/src/sample_player/mod.rs:107–113`, `src/convolution/mod.rs:541–543`).

Existing guarded tests publish and consume on the same thread, warming that
thread before checking allocations (`oscen-lib/tests/realtime_safety.rs:78–114`).
They do not establish the actual fresh-audio-thread contract.

Separately, preallocating `EventPayload::Object(Arc<...>)` on a worker does not
prevent its final-reference destructor/deallocation from running during audio-side
queue draining. Object events have no retirement protocol
(`oscen-lib/src/graph/types.rs:191–224`; `oscen-macros/src/lib.rs:329–338`).
Even `raw_midi_event` allocates for messages shorter than three bytes
(`oscen-lib/src/midi.rs:299–307`).

The flagship FM plugin explicitly locks and allocates in its callback:
`examples/fm-synth/src/lib.rs:223,251–268`. Other examples retain similar patterns.
These are concrete integration violations, not hypothetical lock contention.

**Recommendation:**

- Use an ownership-transfer primitive with bounded, allocation-free first-use and
  steady-state behavior. Audio-thread prewarming is only a tactical mitigation.
- Consider polling asset updates at a documented callback/control boundary rather
  than every sample; this changes activation timing and needs a contract.
- Use inline MIDI/events or stable, explicitly retired object handles. Define what
  can be destroyed on the audio thread, not only what can be allocated there.
- Give the callback exclusive DSP ownership; share control messages/handles, not a
  lock-protected mutable processor. Migrate examples to the existing inline MIDI API.
- Make dropped-event accounting and note-off/panic recovery observable without
  logging or allocating in the callback.

**Gate:** fresh-thread tests with the producer alive elsewhere; allocation **and
deallocation/destructor** instrumentation; overflow paths; actual shared host
adapter callbacks. Also clarify retirement backpressure: the public
`Consumer::retire` drops a rejected ring push (`handoff/mod.rs:80–85`); no normal
built-in consumer overflow was reproduced, but SPSC alone is not a sufficient
proof for arbitrary public consumer usage.

### C6. Parameter reflection constructs heavyweight DSP state

**Reproduced by an existing test. High priority for reliable initialization.**

`param_descriptors_survive_small_stack_thread` requests descriptors on a 128 KiB
thread. A child contains 256 KiB of buffer storage, and the process aborts with
stack overflow (`oscen-lib/tests/review_fixes.rs:93–155`). Metadata initialization
constructs a child on the requesting thread inside `OnceLock::get_or_init`
(`oscen-graph-compiler/src/codegen/emit_params.rs:159–167,248–252,356–362`).

Replacing a whole-graph probe with individual-child probes reduced the problem;
it did not establish a stack bound. The corresponding small-stack safety comment
at `emit_params.rs:243–247` is false for this case.

Architecturally, the deeper issue is that discovering a schema/default constructs
live DSP machinery. Constructors can acquire resources or have side effects;
separately probing and constructing the real graph can yield different defaults.

**Recommendation:** separate lightweight configuration/default metadata from DSP
storage. Use a schema for static facts and an instance snapshot for values that
actually depend on construction. A large-stack worker can mitigate current host
initialization, but increasing this test's stack or wrapping construction in
`Box::new` is not a general architectural fix.

### C7. Public state and meaningful-looking syntax overpromise

These are separate issues with the same UX consequence: the obvious operation is
accepted but does not have the meaning the surface suggests.

- **Reproduced:** `graph.level.set_with_ramp(1.0, 4)` leaves current value at zero
  after four frames, while `graph.set_level(1.0)` reaches one. Public ramp storage
  bypasses the private `active_ramps` invariant
  (`codegen/mod.rs:1057–1073,1099–1133,1358–1369`). Encapsulate mutable ramp internals
  or remove redundant accounting; do not expose operations that silently stall.
- **Reproduced:** `external impulse: DoesNotExist; impulse -> reverb.typo;`
  compiles and installs the convolver asset consumer. The external type and named
  destination are not consulted by emitted installation
  (`ir/lower.rs:728–784`, `codegen/emit_struct.rs:504–533`). Validate the single
  supported asset endpoint and type, or simplify the syntax to match reality.
- **Source-confirmed:** heterogeneous top-level stream types trigger an
  unconditional offline-render `compile_error!`, even when no offline API is
  requested (`codegen/mod.rs:734–741,1602`). Omit the unsupported `BlockRender`
  implementation rather than rejecting an otherwise usable realtime interface.
- **Source-confirmed:** asset loaders receive base graph rate while oversampled
  nodes receive scaled rate (`codegen/emit_struct.rs:480–495,558–565`). Preparing a
  sample/IR for an oversampled consumer needs its effective timebase, or that
  binding must be rejected explicitly. No asset-duration probe was run.

## 3. Performance assessment

### Measured whole-application behavior

Environment: a two-vCPU KVM VM reporting AMD EPYC 9554P, x86-64;
`rustc 1.95.0-nightly (eda76d9d1 2026-01-21)`, LLVM 21.1.8, pinned toolchain
`nightly-2026-01-22`, default Cargo release/bench settings, 48 kHz graph rate.
Criterion used 20 samples, one-second warmup, one-second measurement, with no
concurrent compilation. These are short-run VM measurements, not production
callback deadlines or cross-machine comparisons.

| Existing benchmark | Approximate Criterion estimate per 512 frames |
|---|---:|
| `app/idle/voices8` | 328 µs |
| `app/idle/voices16` | 670 µs |
| `app/chord/voices8` | 387 µs |
| `app/chord/voices16` — same eight-note chord | 772 µs |
| `app/midi_stream/voices8` | 418 µs |
| `app/automation/voices8` | 432 µs |
| `block/events/silent512` | 20.2 µs |
| `block/events/burst16` | 45.9 µs |
| `block/events/midiflood` — 128 events | 80.7 µs |

Two separate sanity runs black-boxed the **whole output slice**, not just the last
sample: each used three 2,000-block batches after 100 warmup blocks. Observed
batch-average ranges were idle8 326–340 µs, chord8 386–405 µs, idle16 652–683 µs,
and chord16 756–836 µs. These support the configured-voice/idle-cost observation
despite benchmark-harness caveats, while also showing VM timing variation.
They are not a statistically rigorous replacement for Criterion.

### P1. Define voice activity before optimizing voice execution

Every array element processes unconditionally
(`oscen-graph-compiler/src/codegen/emit_node.rs:375–403`). `is_active` exists but
is not consulted by the graph scheduler. Voice release is not a tail-completion
protocol (`oscen-lib/src/voice_allocator.rs:100–107`).

This is the first structural throughput opportunity I would pursue, after the
correctness fixes. Define voice states such as awake/releasing/asleep, wakeup on
relevant events, explicit output clearing, release-tail completion, and phase
continuity policy. A zero envelope does not prove an oscillator, filter, delayed
feedback path, or effect may safely stop advancing. Opt-in sleep semantics may
intentionally differ from today's continuously running oscillators; keep the
existing policy available and test each policy rather than silently changing it.

**Measure:** 0/1/8 active voices in 8/16/32 configured voices, released-to-silence
rather than just never-played idle, retrigger/stealing behavior, and dense events.
Do not optimize the allocator's small bounded event-time search before removing
unnecessary per-frame whole-voice work.

### P2. Separate processor state from host buffers and queue capacity

Measured `size_of` on this target:

| Type | Bytes |
|---|---:|
| `EventPayload` | 24 |
| `EventInstance` | 32 |
| `EventInput` | 4,104 |
| `AdsrEnvelope` | 4,208 |
| `MidiVoiceHandler` | 12,328 |
| `VoiceAllocator<8>` | 41,240 |
| Benchmark `FmVoice` | 23,344 |
| Benchmark `PolySynth8` | 347,384 |
| Benchmark `PolySynth16` | 665,592 |

Every event endpoint owns 128 full event slots, even for scalar gates. Every
stream boundary owns a 512-frame array, even in nested graphs driven only through
scalar processing (`oscen-lib/src/graph/types.rs:16–23`;
`oscen-graph-compiler/src/codegen/mod.rs:1371–1421`). The handler's storage is
almost entirely its three queues, not its note/frequency state.

A promising abstraction is a compact processor core plus a host block-I/O wrapper.
Keep static dispatch; it does not require duplicating host staging in every voice.
Genuinely typed inline event storage and configurable capacities are worth
experiments. Do not just lower every queue's capacity: the current 128-note panic
burst requirement must still hold. Object size alone is not proof of cache misses;
measure hot-state layout, construction stack usage, and throughput together.

### P3. Treat block scheduling as a measured capability, not a blanket rewrite

`process_block` loops through the shared frame body, including event-split runs
(`codegen/mod.rs:884–887,1008–1020`). This gives clear sample semantics but no
explicit facility for a node to exploit a contiguous span or amortize asset
polling/control work. Simple inlined loops may still vectorize; it is incorrect
to conclude that all vectorization is impossible from sample-major code alone.

I would not implement `docs/BLOCK_FISSION_SPEC.md` wholesale yet:

1. Maximal feedforward merging can recreate today's loop; smaller regions add
   buffer traffic. The profitable boundary is workload-dependent.
2. `[Voice; N]` is array-of-structs. Corresponding state fields are separated by a
   voice-sized stride; independent voices do not automatically yield unit-stride
   SIMD math. The measured voice is about 23 KiB, not one scalar lane.
3. A source producing one event per sample can generate 512 events per block
   without overflowing a 128-event **per-frame** queue. A new 128-event boundary
   **per-block** queue would drop events and violate the claimed equivalence.
4. `SignalProcessor` permits arbitrary Rust behavior. Reordering across nodes
   assumes dependencies and observable effects are confined to declared inputs
   and node state. State that requirement or introduce an opt-in capability;
   ordinary arbitrary node side effects do not satisfy a blanket proof.
5. Feedback delay, ramp timing, event ordering, and atomic subgraph semantics
   must stay exact. Changing the execution order is not merely an emitter cleanup.

First benchmark an optional span kernel / small same-rate feedforward plan with
scalar fallback. The draft already acknowledges the merge-policy tradeoff and
avoids boundary-event buffering in its initial processing phase; those limited
experiments are reasonable. Test the simplest vectorizable chain, a stateful
filter chain, voice arrays, and feedback-heavy cases. Introduce buffer lifetime
reuse only if buffer size/traffic becomes a measured problem. Keep the existing
scalar path as the semantic reference.

### P4. Add latency-tail and synchronization benchmarks

The handoff's idle per-sample synchronization needs an installed-but-unchanged
asset benchmark; playback without an installed consumer does not measure it.
Vary active consumers and unrelated arc-swap-using threads. No steady-state cost
attribution was established by the allocation probe.

Convolution concentrates FFT/partition work at boundaries, processes channels on
matching schedules, and temporarily processes two engines during swaps
(`oscen-lib/src/convolution/mod.rs:193–241,335–391,559–578`). Oscilloscope triggering
can copy a configured capture window in one processing step
(`oscen-lib/src/oscilloscope/mod.rs:111–122,265–279`). Both are bounded yet potentially
bursty. These are **latency hypotheses**, not observed xruns.

Measure callback distributions and maxima at 16/32/64/128/512 frames, multiple
instances/channels, long IRs, and swaps/captures. Do not use average throughput to
assert a callback meets its deadline.

### Benchmark hygiene before claiming optimization wins

- `per_sample.rs:77,93,110` black-boxes `process()`'s unit result, not an output.
  It was **not fully optimized away** in this run (approximately 11.5 ns/simple
  and 38.3 ns/complex), but it does not make all intended DSP observable.
- That complex fixture leaves two oscillator branches disconnected from the mix
  and never gates its envelopes (`per_sample.rs:21–66`). Add fixture sanity checks.
- The passthrough sample/block comparison times sine generation and a black box
  per sample only on the sample side (`graph_blocks.rs:50–73`). Its delta is not
  isolated block overhead, despite the comment.
- Make complete output buffers observable, prepare identical input outside timed
  regions, verify all intended branches contribute, and distinguish stable,
  sparse-automation, dense-automation, and audio-rate modulation workloads.
- Track compiler expansion time, rustc time, generated token/binary size, and
  processor size for scaled graphs. No compile-time benchmark was run in this
  review. Repeated edge scans in codegen are a plausible scaling concern
  (`emit_node.rs:61–72,193–227`), not a demonstrated bottleneck.

## 4. Elegance: three boundaries would simplify the project

### A. An authoritative analyzed graph plan

Do not replace the current IR. Add the missing resolved facts and a small planning
stage so codegen does not reinterpret syntax:

- endpoint kind/direction and payload requirements, including a principled Rust
  dispatch fallback where macro-time metadata is unavailable;
- separate node element / frame channel addressing;
- one driver group per destination, with explicit sum/append/reject semantics;
- expression clock, dependencies, and scheduling phase from all referenced nodes;
- explicit event merge/clear/collect operations and conversion boundaries.

Lower single/list/wildcard hoists to the same resolved declarations. Consume one
expression visitor throughout analysis and emission. Invalid semantic combinations
should return `Err(Diagnostics)`, not `Ok(tokens_containing_compile_error!)`
(the latter exists in `emit_node.rs:219–224`). This also makes the compiler library
more useful to future diagnostics/inspection tools.

This is a targeted semantic refactor, not a request for SSA, a generic pass
framework, or a new compiler backend.

### B. Lightweight schema/configuration versus live processor state

Separate schema, initial configuration, and current state. Keep generated parameter
IDs and reflection, but do not derive a static schema by instantiating a large
processor. Make current/target values explicit for ramps, preserve stable host
identities independently of internal declaration-order indices, and protect
scheduler bookkeeping behind generated setters.

Likewise, distinguish an internal node's scalar ports from a host's block port
buffers. A homogeneous offline helper should be an optional adapter, not a global
restriction on channel layouts.

### C. Audio processor versus controls/host adapter

The core owns DSP state exclusively. A control handle owns loaders, publication,
and lightweight parameter/control delivery. An adapter owns host block chunking,
event offsets/overflow policy, output collection, and host parameter mapping.
These can all be ordinary Rust types; no new DSL syntax is necessary initially.

The existing sample-player example extracts its real loader by replacing a graph
field with a dummy handle (`examples/src/bin/sample_player.rs:66–70`). An explicit
one-time extraction or build/split API would express that ownership honestly and
could define how detached loaders track later sample-rate changes.

Share the FM synth graph between plugin and standalone before generalizing an
adapter crate. They currently duplicate topology, parameter identity, and UI
routing (`examples/fm-synth/src/lib.rs:22–130`, `src/main.rs:22–50,52–194`). The
registry is implemented, but these applications are not yet getting its full
single-source-of-truth benefit.

## 5. DSL UX: prioritize reliable composition over terseness

The best existing features are explicit endpoint kinds, visible connection
expressions, comma fan-out, hoists, parameter metadata, and explicit feedback
delays. They remove repetitive wiring without hiding the audio model.

The next UX work should make equivalent spellings behave equivalently, then make
unsupported forms fail clearly. The following are source-confirmed unless marked
otherwise; they were not all independently compiled by the review probes.

### Unify hoists, including metadata

Single, list, and wildcard hoists should differ only in selection/renaming.
Currently list expansion can inherit type information only if a wildcard elsewhere
happened to request a manifest, and it does not inherit the same ramp/spec metadata
(`oscen-graph-compiler/src/ir/lower.rs:98–148`, `src/manifest.rs:64–116,696–795`).
Refactoring a wildcard into a renamed list should not change parameter type,
range, or smoothing.

Keep the companion-manifest mechanism for now, but contain its naming/hygiene
cost. Selective imports, aliases, re-exports, custom payload type names, and
factory constructors deserve first-class examples and tests. Provide an explicit
type/manifest-path escape hatch rather than requiring users to debug continuation
macro names. Do not blindly make all nodes require wildcard-style manifest imports.

### Make expression syntax internally consistent

Method-call arguments are opaque Rust while free-function arguments are graph
expressions (`src/ir/expr/mod.rs:61–75`, `src/parse.rs:1455–1476`). Thus
`x.clamp(lo, hi)` and `clamp(x, lo, hi)` do not resolve endpoint arguments the same
way. Parenthesized expressions and function calls return before a common postfix
chain (`src/parse.rs:1345–1405`); defaults accept a narrower grammar than their
`syn::Expr` representation implies (`:921–946`).

A small atom → postfix → arithmetic parser, shared argument semantics, and a
clearly delimited Rust-expression escape for constants/defaults would be more
elegant than additional special cases. Distinguish graph references from Rust
constants explicitly where ambiguity exists.

### Make annotations meaningful and constructors extensible

Stream annotation handling recognizes particular type names and otherwise falls
back to `f32` (`src/codegen/mod.rs:148–173`). Frame constructor width syntax can be
accepted then discarded (`src/parse.rs:1370–1388`). Honor such annotations or
reject them; accepting ignored syntax is worse than a smaller honest language.

A modest proposed escape hatch is `node voice: Voice = make_voice();`. It would
supply the type information codegen/manifest lookup needs without dictating
constructor naming. This is **proposed syntax**, not currently supported. The
current missing-type path can omit a node field and leave rustc to diagnose
missing fields (`src/codegen/mod.rs:1410–1425`). Report the problem earlier.

### Improve the external-node author experience

- Complete a node-author prelude (`Node`, `SampleRate`, event types), or explicitly
  distinguish it from the graph-consumer prelude.
- Route generated `ArrayVec` references through a hidden Oscen re-export rather
  than requiring downstream users to depend directly on `arrayvec`
  (`oscen-macros/src/lib.rs:330–333`).
- `EventInput<T>` / `EventOutput<T>` should either enforce payload compatibility
  or lose misleading generic parameters. They currently store the same untyped
  queue and allow connections across differing `T`
  (`oscen-lib/src/graph/types.rs:286–334`, `src/graph/static_context.rs:144–154`).
- Keep expansion support available but hide marker/CPS machinery from the normal
  conceptual API. Keep plain data, fields, and the DSP function prominent.

### Diagnostics and documentation

The accumulated-error design and cycle-path diagnostics are strengths. Preserve
exact offending spans/reasons when lowering endpoint references instead of
collapsing them to an `Option` and a whole-statement resolution error
(`src/ir/lower.rs:620–643,1850–2014`). Stabilize error ordering and test recovery
across missing semicolons, not just malformed complete statements.

Promote cookbook examples to compiled fixtures. The opt-in NIH params test is
already stale enough to fail parsing, and `ERGONOMICS_PLAN.md` claims `poly::<N>`
is shipped while the implementation/examples use manual wiring. Test the public
teaching surface rather than adding another prose-only tour of syntax.

I would defer new `poly` syntax, implicit endpoint chaining, and broader wildcard
sugar until the existing combinations above are reliable. A shared Rust voice
component/host adapter can reduce application repetition sooner and provides a
concrete basis for deciding which future sugar is actually useful.

## 6. Roadmap and acceptance criteria

| Milestone | Scope | Completion evidence |
|---|---|---|
| 1. Restore trustworthy behavior | C1–C7 targeted fixes, existing failing tests, callback inline MIDI/exclusive ownership | Normal core and enabled-feature tests green; permanent regression tests for reproduced cases; cold-thread and destruction checks |
| 2. Consolidate semantic contracts | Resolved driver/address/clock plan, unified hoists, truthful lifecycle, protected ramps | Refactoring-equivalence tests; compiler errors returned as diagnostics; no regression in golden audio or compiler scaling |
| 3. Make integration exemplary | Shared FM core, schema/config split, control-handle extraction, host event collection | Plugin/standalone share topology and param IDs; adapter-level RT checks; arbitrary host block sizes and event timestamps covered |
| 4. Reduce idle/storage cost | Opt-in voice lifecycle, compact processor/host buffer separation, event-capacity experiments | 0/1/N voice scaling, release-tail/retrigger correctness, measured struct/stack sizes and throughput, explicit phase policy |
| 5. Selective block optimization | Small opt-in span/region experiment with scalar fallback | Same sound/timing for promised-equivalent modes; no new event overflow; wins on representative fixtures without feedback regressions |

Do not combine all of these into one branch. Land repro regressions and narrow
fixes first, then consolidate their shared machinery. Fixing one routing bug at a
time indefinitely without an authoritative plan would also be a mistake.

### Test strategy that pays for the refactor

Add metamorphic tests: introduce a graph boundary, rename/qualify a node, replace
a wildcard with equivalent explicit hoists, add an identity expression, or change
block segmentation. Where the declared semantics are unchanged, output and
diagnostics should remain equivalent. Do not demand equivalence where an explicit
resampling or voice-phase policy intentionally differs.

Cross the feature axes: known/unknown kind; scalar/array/index/channel;
stream/value/event; same/cross rate; sample/block; nested/top-level; active/idle;
zero/burst/overflow events. Property tests over small generated graphs could cover
this more efficiently than an ever-growing set of isolated token snapshots.

## 7. Reproducing this review

`docs/review-probes/` is a standalone, non-published diagnostic crate, deliberately
outside the production workspace. It prints observations and expected contracts;
it is **not** a regression suite asserting that bugs should remain. Promote cases
to normal tests when fixing them. Its runtime allocator counters are for the probe
process only, not a production instrumentation recommendation.

From the repository root, copy the reviewed dependency lock into the scratch crate
before its first run so dependency resolution matches the application:

```bash
cp Cargo.lock docs/review-probes/Cargo.lock
cargo run --release --manifest-path docs/review-probes/Cargo.toml --target-dir target --bin api
cargo run --release --manifest-path docs/review-probes/Cargo.toml --target-dir target --bin dsl
cargo run --release --manifest-path docs/review-probes/Cargo.toml --target-dir target --bin runtime
cargo run --release --manifest-path docs/review-probes/Cargo.toml --target-dir target --bin bench_sanity
```

Run `runtime` as a fresh process. Its producer stays alive on the main thread,
its consumer starts on a new thread, and counting excludes spawn/join and
formatting. The object-event payload is created before processing is counted.

Existing benchmark commands used in this review (run serially):

```bash
cargo bench -p oscen --bench per_sample -- --warm-up-time 1 --measurement-time 1 --sample-size 20
cargo bench -p oscen --bench graph_blocks -- --warm-up-time 1 --measurement-time 1 --sample-size 20 --save-baseline architecture-review
cargo bench -p oscen --bench synth_app -- --warm-up-time 1 --measurement-time 1 --sample-size 20 --save-baseline architecture-review
```

For real performance decisions, harden the harness as described above, use longer
runs, record callback distributions, and repeat on the target audio hardware.

## Bottom line

Oscen has the right overall direction. Its biggest improvement is not a shorter
syntax or a different dispatch mechanism: it is **making the static semantics,
public API, and ownership boundaries agree**. That consolidation would simultaneously
improve sound correctness, make the DSL more predictable, simplify codegen, and
create a much safer foundation for the genuinely promising performance work.
