# Oscen Cookbook

Idioms for building real synths/plugins with `graph!`. Everything here was
learned the hard way by downstream projects; read this before writing a new
plugin, and keep it updated when you discover a new gotcha.

## Parameters

### Hoist child endpoints instead of hand-forwarding

`input <node>.<endpoint> [rename] [= default [spec]];` declares a graph
input *and* wires it to the child endpoint — one line replaces the
declare + connect + setter-forwarding pattern:

```rust
graph! {
    name: MySynth;
    nodes {
        osc    = PolyBlepOscillator::saw(440.0, 0.6);
        voices = [Voice::new(); 8];
        inner  = FilterSection::new();     // another graph! type
    }

    input osc.frequency;                   // input named `frequency`,
                                           // default inherited from the ctor (440.0)
    input osc.amplitude level = 0.5 [0.0..1.0, group = "Mix"];  // rename + metadata
    input voices.cutoff;                   // array: broadcasts to all 8 voices
    input inner.resonance;                 // nested graph!: re-export its input
    input parser.midi_in: event;           // non-value kinds need the annotation

    // Endpoint lists with a `*`-substitution rename pattern — the fix for
    // per-branch param duplication (env_a_attack, env_b_attack, …):
    input branch_a.{env_attack, env_decay, env_sustain} a_*;
    input branch_b.{env_attack, env_decay, env_sustain} b_*;
    // Patterns: `prefix_*`, `*_suffix`, or `pre_*_post`. No `= default [spec]`
    // on list hoists — hoist singly when you need per-param metadata.
    ...
}
```

Rules of thumb:

- Without `= default`, the initial value is read from the constructed child
  node — the child stays the single source of truth.
- A hoist is a real input: it gets setters, ramp plumbing (`ramp:` in the
  spec), a param-registry entry, and `nih_params` treatment like any other.
- Hoisting through a nested `graph!`'s ramped input drives the child's ramp
  state directly (no double smoothing); put the ramp at whichever level you
  hoist from, not both.
- Name collisions (hoist vs. declared input) are duplicate-declaration
  errors; rename the hoist.

### Wildcard hoists (`input node.*;`)

Hoist *every* input endpoint of a node in one line — value, stream, and
event kinds alike:

```rust
graph! {
    name: MySynth;
    nodes {
        voices = [FMVoice::new(); 8];      // derive(Node) type or graph! type
    }

    input voices.*;                        // hoist everything not otherwise claimed
    input voices.cutoff bright [0.0..1.0]; // explicit hoist wins; wildcard skips it

    connections {
        handlers.frequency -> voices.frequency;   // wired manually → skipped too
        voices.audio_out -> out;
    }
}
```

Semantics:

- Expands to the node's **input** endpoints in the child's declaration
  order, so param-registry ordinals are stable as long as the child is.
- **Skips** endpoints that are connection destinations for that node
  anywhere in the graph, endpoints already hoisted explicitly, and `asset`
  inputs (those bind via `external`).
- No rename pattern, default, or `[spec]` on a wildcard — hoist an endpoint
  individually to rename it or attach metadata (the wildcard then skips it).
- A collision between an expanded endpoint and any other declaration is a
  hard error on the `input node.*;` line; rename the other declaration or
  hoist that endpoint explicitly with a rename.
- Array nodes broadcast, same as single hoists.
- Nested `graph!` types hoist through: the inner graph's declared inputs
  (including its own hoists) are what the wildcard sees.

How it works, and the gotchas that follow: a proc macro can't enumerate
another type's fields, so `#[derive(Node)]` and `graph!` each export a
hidden "endpoint manifest" macro named `__oscen_endpoints_<TypeName>`
alongside the type, and `input node.*;` resolves the node's constructor
path to that manifest at expansion time (`a::b::FMVoice::new()` →
`a::b::__oscen_endpoints_FMVoice!`). Consequences:

- **The manifest must be reachable where the node type is named.** A
  qualified constructor path (`child_crate::FMVoice::new()`) or a glob
  import (`use child_crate::*;`) both work — the manifest re-export
  travels next to the type. With a *selective* import (`use
  child_crate::FMVoice;`) the bare manifest name is not in scope: also
  import it (`use child_crate::__oscen_endpoints_FMVoice;`) or qualify the
  constructor. The failure mode is rustc's "cannot find macro
  `__oscen_endpoints_FMVoice`" at the `graph!` call site.
- **Manifest names derive from the type name alone**, so two same-named
  node types in one crate collide on the exported macro (duplicate
  `macro_rules!` definition), and same-named types from different crates
  need path-qualified constructors to pick the right manifest. Rename one
  type if you hit this.
- Only the node type's `pub` endpoint fields appear in its manifest (a
  parent graph writes child fields directly; privacy applies).
- The node's type must be visible syntactically (`T::new()`,
  `path::T::new()`, `[T::new(); 8]`). A bare constructor call
  (`node = make_voice()`) can't be resolved — the compiler tells you to
  name the type or hoist individually.

### Declare metadata once, consume it everywhere

Every graph with `value` inputs gets a generated parameter registry:

```rust
graph! {
    name: MySynth;
    input cutoff: value = 1000.0 [20.0..20000.0, log, unit = "Hz", ramp: 64];
    input drive:  value = 1.0    [0.0..10.0, center = 2.0, group = "Tone"];
    ...
}

// Generated alongside the struct:
MySynthParam::Cutoff                     // Copy id enum, declaration order
MySynthParam::ALL, MySynthParam::COUNT
MySynthParam::from_name("cutoff")        // Option<MySynthParam>
MySynth::param_descriptors()             // &'static [ParamDescriptor]
graph.set_param(p, v)                    // RT-safe, uses declared ramp
graph.set_param_immediate(p, v)          // RT-safe, snaps
graph.get_param(p)                       // reads the target value
```

Use the enum, not per-param plumbing:

- **UI → audio thread**: send `(MySynthParam, f32)` over your channel and
  drain it in the callback with `graph.set_param(p, v)`. Do not write a
  hand-rolled `enum ParamChange { Cutoff(f32), … }` with one match arm per
  parameter.
- **Presets**: store `&[(MySynthParam, f32)]` (or names via `from_name` for
  durable serialization) and apply in a loop with `set_param_immediate`.
  Validate against `descriptor().range` at load time.
- **Generic UIs / tooling**: iterate `param_descriptors()` for names,
  defaults, ranges, units, groups.

`param_descriptors()` builds its table lazily on first call — call it once
during setup, **off** the audio thread. The enum dispatchers are
allocation-free and audio-thread safe.

### Ramps

`ramp: N` is in **frames** (2205 ≈ 50 ms at 44.1 kHz — it does not adapt to
sample rate yet). A ramped input generates `set_x` (default ramp),
`set_x_with_ramp(v, frames)`, and `set_x_immediate`. After construction,
ramps start from the declared default; if you apply a preset before playing,
run a few `process_block` calls to settle ramps before asserting on output.

## Events & MIDI

### Pushing events from the host

Every graph-level `input x: event;` gets a generated push helper. Use it:

```rust
graph.push_midi_in([0x90, note, velocity], frame_offset);  // note on
graph.push_midi_in([0x80, note, 0], frame_offset);         // note off
graph.push_trigger(0.5f32, 0);                             // scalar event
```

`[u8; 3]` converts to `EventPayload::Midi` and `f32` to
`EventPayload::Scalar` — both plain data, **no heap allocation**, safe on
the audio thread. The helper returns `false` if the queue (128 events per
endpoint per block) is full.

**Never** construct `EventPayload::Object(Arc::new(RawMidiMessage::new(..)))`
on the audio thread: `Arc::new` heap-allocates. `Object` payloads are for
non-RT contexts or preallocated objects only. `MidiParser` accepts the
`Midi([u8; 3])` representation natively.

`frame_offset` is relative to the start of the *next* `process_block` call.

### Chunking host buffers

`process_block(frames)` requires `frames <= Graph::MAX_BLOCK_SIZE`. Hosts
routinely hand you more. Chunk, and rebase event offsets per chunk:

```rust
let mut start = 0;
while start < num_samples {
    let end = (start + MySynth::MAX_BLOCK_SIZE).min(num_samples);
    // push this chunk's events with offset rebased: (t - start) as u32,
    // clamping stragglers into the final chunk
    graph.process_block(end - start);
    // copy graph.out_block[..end - start] to the host buffer at [start..end]
    start = end;
}
```

When converting host note events (e.g. nih-plug `NoteEvent`) to MIDI bytes,
round velocity with `(v * 127.0).round() as u8` — truncation audibly drops
full-velocity notes to 126.

## Connections

- **Fan-in sums**: several stream sources into the same destination add
  (`branch_a.output -> mix.input; branch_b.output -> mix.input;`), Cmajor
  semantics. Array outputs wired to a graph output also sum.
- **Comma fan-out**: `freq -> osc_a.frequency, osc_b.frequency;` is one
  statement per destination. Not combinable with a `-> […] ->` delay
  bracket (each destination would need its own delay).
- **Expressions**: `osc.output * gain * 0.3536 -> out;` — keep gain staging
  and headroom visible inside the graph instead of post-processing buffers.

## Feedback & cycles

A plain-`->` cycle is a compile error; the diagnostic names the cycle path.
Two legal ways to close a loop:

```rust
osc.output -> [1] -> mixer.feedback;        // 1-sample inline delay
osc.output -> [my_delay] -> mixer.feedback; // route through a declared Delay
```

**Subgraphs are scheduled atomically.** If `sub.some_output` feeds a node
that also feeds `sub.some_input`, the parent sees a cycle even when the
output doesn't actually depend on that input internally. Until per-output
dependency tracking lands, either break the false cycle with `-> [1] ->`
(one sample of latency on that edge) or restructure so the tightly-coupled
nodes live in the same graph level.

## Voices / polyphony

The standard chain is `MidiParser -> VoiceAllocator -> MidiVoiceHandler[N]
-> Voice[N]`, with the voice array wired to a graph output:

- An array output wired **directly to a graph output** is summed
  automatically. Summing at other destinations is not yet general — keep
  the sum at the boundary.
- Apply headroom when copying the sum out (≈ `1/sqrt(N)` for N voices), or
  the first fortissimo chord will clip.
- Big voice arrays make big structs: graphs live by value, so spawn audio
  threads with a large stack (e.g. 8 MB) and keep test-thread stacks in
  mind (`std::thread::Builder::new().stack_size(..)` in tests).

## Node authoring

- Node types used in `graph!` must be in scope at the macro site; keep the
  `use` even if rustc calls it unused (`#[allow(unused_imports)]`).
- Downstream crates must keep `arrayvec` (and `paste`, if used by generated
  code) in their own `Cargo.toml`: macro-generated code references
  `::arrayvec::ArrayVec`.
- Real-time safety is a hard requirement in `process()` and everything it
  calls: no allocation, locking, or blocking. `oscen-lib/tests/
  realtime_safety.rs` enforces this with `assert_no_alloc` — add coverage
  there when you add hot-path API.
- Audio ↔ UI communication: use the existing `handoff` / ring-buffer /
  `arc-swap` patterns; never channels or mutexes on the audio side.

## Testing a synth

The proven recipe (see third-stone's `release_tail.rs`): initialize, apply a
preset, settle ramps, send a note-on, measure held RMS, send note-off, then
assert per-window RMS decays to silence. Assert `s.is_finite()` on every
sample — NaNs propagate silently otherwise. Run heavy render tests with
`--release` and a large test-thread stack.
