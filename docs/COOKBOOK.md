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
- Name collisions (hoist vs. declared input, output, node, or `external`)
  are duplicate-declaration errors; rename the hoist.
- A hoist is the endpoint's *driver*: it synthesizes `name ->
  node.endpoint`. Hoisting an endpoint that also has an explicit
  connection (or a second hoist) is a duplicate-driver compile error —
  remove one.
- A typed value endpoint (see "Typed value endpoints" below) hoists with
  the annotation: `input filter.mode: value: FilterMode;`. Wildcards
  infer the type from the manifest; explicit single hoists always need
  the annotation.

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
- Endpoint metadata is inherited from the child: a stream endpoint typed
  `Frame<2>` hoists as a `Frame<2>` input, a typed value endpoint (a
  `ValuePayload` field like a `FilterMode` enum) hoists as an input of
  that type (typed setter, no param-registry entry), a child `graph!`
  input declared `[ramp: N]` hoists as a parent input with the same
  `[ramp: N]` (the parent ramps; the child follows per frame), and a
  child `graph!` input's param spec (range, `log`, `unit`, `center`,
  `step`, `group`, display name) survives into the parent's param
  registry. `#[derive(Node)]` children carry no param spec (the derive
  has no attribute surface for one). One
  exception: a
  `#[derive(Node)]` child that stores a value input as a `ValueRampState`
  field ramps with a length known only at runtime, which the wildcard
  cannot reproduce — it errors and tells you to hoist that endpoint
  explicitly with `input node.endpoint [ramp: N];` (the wildcard then
  skips it).
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
- **Globs and locally-defined nodes don't mix on bare paths.** Under
  `use oscen::prelude::*;` (or any glob that exports manifest aliases), a
  node type you `#[derive(Node)]` in the *same crate* can't be
  wildcard-hoisted by its bare name: the derive's manifest alias is a
  macro-expanded name, and rustc refuses to resolve macro-expanded names
  alongside glob imports — "cannot find macro `__oscen_endpoints_T`", or
  E0659 "ambiguous name" if your type also shadows a prelude name
  (`Gain`, `Oscillator`, `Delay`, `Value`, …). Escapes, any one of:
  qualify the constructor path (`dsp::Gain::new()` — put your node types
  in a module), rename your type, or replace the glob with selective
  imports.
- **Manifest names derive from the type name alone.** The crate-global
  `#[macro_export]` behind the manifest also hashes the definition's
  tokens, so two same-named node types in *different modules* of one crate
  coexist; only two byte-identical same-named definitions in one crate
  still collide on the exported macro (duplicate `macro_rules!`
  definition). Same-named types from different crates need path-qualified
  constructors to pick the right manifest. Rename one type if you hit
  either case.
- **What the manifest carries.** Every endpoint's name and kind, plus
  optional metadata: the declared type of non-f32 endpoints (`ty = …` —
  the frame type of non-mono streams like `Frame<2>`, and the payload
  type of typed value endpoints like `FilterMode`, so hoists don't
  collapse to mono/f32), the declared ramp length of ramped `graph!`
  value inputs (`ramp = N`), a `ramped` marker for `ValueRampState`
  fields on derive types (length unknown at compile time), the param
  spec of `graph!` value inputs (`range = a..b`, `log`, `center`,
  `unit`, `step`, `group`, `display`), and visibility markers (`priv`,
  `restricted`). Range/center/step ride as raw expression tokens
  resolved at the parent's call site — a range referencing a
  child-crate-private const won't resolve in the parent (same hygiene
  caveat as `ty = …`).
- Endpoint-field visibility: *private* fields (no `pub`) are listed with
  `priv` and wildcards skip them — a parent graph writes child fields
  directly, so privacy applies; the marker tells "present but not `pub`"
  apart from "missing". `pub(crate)`/`pub(super)` fields are marked
  `restricted` and hoist normally — the common case is a same-crate
  parent; a cross-scope hoist fails with rustc's own "field is private"
  error at the hoist line.
- Frame-type tokens from a `graph!` child are fully qualified
  (`::oscen::frame::Frame<2>`) and resolve anywhere. A `#[derive(Node)]`
  child carries the field's *literal* type tokens, which resolve at the
  parent's call site — a child field typed with a bare `Frame<2>` (or any
  custom frame or `ValuePayload` type) requires that name to be in scope
  where the parent `graph!` is written. Typed value tokens from a `graph!`
  child are literal too (`FilterMode`, however the child spelled it) —
  same rule.
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
allocation-free and audio-thread safe. Graphs with hoist-inherited
defaults construct a probe instance to read the defaults back; that build
runs on an internal big-stack thread, so calling `param_descriptors()`
from a small-stack host thread (e.g. nih-plug's `Params::default()`) is
safe even for multi-megabyte voice-array graphs.

Input names may not shadow the generated API: an input named `param`,
`sample_rate`, or a stream input named `process` (whose `process_block`
accessor collides with the built-in) is a compile error naming the
collision — rename the input.

### Ramps

`ramp: N` is in **frames** (2205 ≈ 50 ms at 44.1 kHz — it does not adapt to
sample rate yet). A ramped input generates `set_x` (default ramp),
`set_x_with_ramp(v, frames)`, and `set_x_immediate`. After construction,
ramps start from the declared default; if you apply a preset before playing,
run a few `process_block` calls to settle ramps before asserting on output.

### Typed value endpoints

Value endpoints carry any plain data, not just `f32` (Cmajor's "value
endpoints carry any data"). The model is two-tier:

- **`f32` value inputs are params**: automatable knobs with a registry
  entry, descriptors, `set_param` dispatch, ramps, and `nih_params`
  treatment.
- **Every other payload type is a typed value**: structural configuration
  (a filter mode, a waveform selector, a small config struct) that flows
  through the graph as a plain `Copy` — no registry entry, no ramping, no
  resampling.

Declaring them:

```rust
// The payload: plain data, opted in with the empty marker trait
// (which requires Copy + Default + Send + 'static).
#[derive(Clone, Copy, Default, Debug, PartialEq)]
pub enum FilterMode { #[default] Lowpass, Highpass, Bandpass }
impl ValuePayload for FilterMode {}

// On a #[derive(Node)] type: a value field of any ValuePayload type.
#[derive(Node)]
pub struct ModeFilter {
    #[input(value)]
    pub mode: FilterMode,
    ...
}

graph! {
    name: MySynth;
    input mode: value: FilterMode;                 // typed graph input
    input tuned: value: FilterMode = FilterMode::Highpass;  // explicit initial value
    output active: value: FilterMode;              // typed graph output
    input filter.mode: value: FilterMode;          // typed hoist (annotation required)
    connections { mode -> filter.mode; ... }
}
```

Semantics:

- A typed input/output is a **real field of the declared type** with a
  typed setter (`set_mode(FilterMode)`). Without `= default` it starts at
  `T::default()`; a typed *hoist* without a default inherits the child
  constructor's value.
- **Write-wins, latched reads**: connections copy the source field into
  the destination each frame, so the destination always holds the last
  written value — a latch that costs one copy whether or not the value
  changed. There is no interpolation between writes.
- **Excluded from the param registry and `nih_params`** by design: params
  are automatable f32 knobs; typed values are structural config. If you
  want an enum-ish knob the DAW can automate, declare it as an f32 param
  with `step`/labels instead and map it to the enum inside the node.
- **No param spec, no ramp** — `input mode: value: FilterMode [ramp: 64];`
  is a compile error (there's nothing to interpolate). `: f32` is
  normalized away: it declares a plain param, byte-identical to an
  unannotated one.
- **No fan-in**: two sources into one typed endpoint is a compile error —
  values don't sum. (f32 value fan-in is rejected the same way; only
  streams sum.)
- **Cross-rate boundaries latch**: a typed edge into or out of an
  oversampled node copies once per outer tick at the block boundary — the
  inner node sees the latched value for the whole inner block. Only the
  default `[latch]` policy is legal on typed cross-rate edges;
  `[linear]`/`[sinc]` are compile errors.
- **Hoisting**: wildcard hoists (`input node.*;`) inherit the payload type
  through the endpoint manifest, for derive and nested `graph!` children
  alike; explicit single hoists need the `: value: T` annotation. List
  hoists (`input node.{a, b};`) inherit the type only when the node's
  manifest is resolved anyway (a node that also has a wildcard) —
  otherwise they expand as `f32` and a typed endpoint fails
  rustc's `ConnectEndpoints<f32, T>` check; hoist it explicitly with the
  annotation.
- **Type tokens resolve at the parent's call site** (same hygiene caveat
  as frame types): a `#[derive(Node)]` child's manifest carries the
  field's literal tokens, so hoisting `mode: FilterMode` through a
  wildcard requires `FilterMode` to be in scope where the parent `graph!`
  is written.

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

- **Fan-in sums — streams only**: several stream sources into the same
  destination add (`branch_a.output -> mix.input; branch_b.output ->
  mix.input;`), Cmajor semantics. Array outputs wired to a graph output
  also sum. Two sources into one *value* endpoint is a compile error
  (values don't sum); combine them explicitly (`a + b -> amp.gain;`) or
  keep a single source.
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
