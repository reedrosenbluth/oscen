# Oscen Ergonomics Plan

Derived from an audit of **third-stone** (the largest downstream project: a
107-parameter, 8-voice PM synth with plugin + standalone targets).

## The problem, quantified

In third-stone, adding **one parameter** touches ~10 edit sites across 4+
files:

| Site | File | Lines today |
|---|---|---|
| Voice `input` decl + default | `voice/mod.rs` | 107 decls |
| Voice connection to node | `voice/mod.rs` | 116 edges |
| Plugin graph input + metadata | `lib.rs` | 107 decls |
| Plugin identity connection | `lib.rs` | 108 edges |
| Standalone graph input (metadata *dropped*) | `main.rs` | 107 decls |
| Standalone identity connection | `main.rs` | 108 edges |
| `ParamChange` enum variant | `main.rs` | 107 variants |
| Audio-callback match arm | `main.rs` | 108 arms |
| Preset field + `apply_to_graph` setter | `presets.rs` | 107 each |
| UI binding (x2 targets) | `editor.rs` / `main.rs` | 107 each |

`lib.rs` + `main.rs` are ~1,500 lines, of which maybe 100 are logic. The rest
is mechanical forwarding that the compiler already has full knowledge of.

Additional pain points observed:

- Ten `-> [1] ->` delay-breakers in the voice exist only because the
  scheduler treats subgraphs as atomic (no real feedback involved).
- `VOICE_SUM_HEADROOM` lives *outside* the graph because array summing only
  happens when an array output is wired directly to a graph output.
- Param metadata (ranges, units) is coupled to `nih_params`, so the
  standalone target silently loses it.
- `push_midi` in third-stone does `EventPayload::Object(Arc::new(...))` on
  the audio thread — a heap allocation the library forbids — because the
  allocation-free `EventPayload::Midi` variant wasn't discoverable.
- Both `process()` implementations (nih chunking/event rebasing, cpal thread
  setup) are generic boilerplate any oscen synth would need.

---

## 1. Hoisted endpoints (highest impact)

Let a parent graph re-export child endpoints, with deep paths, wildcards,
and rename patterns. Proposed syntax (keeping our `input/output kind name`
shape):

```rust
graph! {
    name: ThirdStoneGraph;
    nih_params;

    input midi_in: event;

    nodes {
        voices = [ThirdStoneVoice::new(); 8];
        ...
    }

    // Hoist every value input of the voice array as a graph input,
    // broadcast to all array elements. Defaults come from the child decls.
    input voices.*;

    // Selective override: attach/replace metadata on a hoisted input.
    input voices.osc_a_pitch [-80.0..70.0, unit = "st", ramp: 2205];

    // Exclusions for endpoints wired manually:
    input voices.* except { frequency, gate };
}
```

And inside the voice, the same mechanism with a `*`-substitution
rename pattern eliminates the per-prefix duplication of `SynthBranch` /
`AdbdsrEnvelope` params:

```rust
graph! {
    name: ThirdStoneVoice;
    nodes {
        branch_a = SynthBranch::new();
        branch_b = SynthBranch::new();
    }
    input branch_a.env_*  env_a_*;     // env_a_attack, env_a_decay1, ...
    input branch_b.env_*  env_b_*;
    input branch_a.*      shaper_a_*  except { env_*, input, gate, ... };
}
```

Semantics:

- Hoisting a **value** input generates the same storage/setter/ramp plumbing
  as a declared input plus the implicit connection. Defaults are inherited
  from the child declaration (single source of truth); an override on the
  hoist site replaces default and/or metadata.
- Hoisting through an **array** node broadcasts (same rule as today's
  `input -> voices.x` fan-out).
- Hoisted **outputs** follow the fan-in/summing rules (§2).
- Name collisions between hoists (or with declared inputs) are compile
  diagnostics with both spans.
- Deep paths (`voices.branch_a.env_attack`) work because a hoist in the
  child re-exports as a first-class endpoint the parent can hoist again —
  no special multi-level machinery needed.

Impact: deletes ~450 lines from `lib.rs`, ~430 from `main.rs`, and a further
~200 from `voice/mod.rs`/`branch.rs`/`filter_section.rs`. "Add a param"
becomes: one `input` line on the node that owns it, one optional metadata
override at the top.

Compiler notes: parse `input <path> [rename] [spec] [except {…}]` as a new
`GraphItem::Hoist`; expand during IR lowering (after child graph interfaces
are known — requires subgraph endpoint tables, which codegen already has via
the generated inherent types). Diagnostics must list the expanded names on
request (`OSCEN_DUMP_GRAPH`, §8).

## 2. General fan-in / fan-out & connection sugar

Make connection rules uniform: multiple sources into one stream destination
**sum**; array↔scalar connections fan in/out; single-endpoint nodes can
omit the endpoint name and chain.

```rust
connections {
    // fan-in summing anywhere, not just at graph outputs:
    branch_a.output -> mix_bus.input;
    branch_b.output -> mix_bus.input;          // sums

    // array -> scalar sums; scalar -> array broadcasts (already partial):
    voices.audio_out -> master_gain.input;

    // chaining through single-in/single-out nodes:
    out_mix.output -> output_gain -> master_gain -> audio_out;

    // comma fan-out:
    osc_b.output -> osc_a.cross_mod, branch_a.ring_mod_in;
}
```

This removes the "arrays only sum when wired directly to a graph output"
special case that pushed `VOICE_SUM_HEADROOM` outside third-stone's graph.
With expression connections already supported, the headroom becomes visible
and graph-internal:

```rust
    voices.audio_out * 0.3536 -> audio_out;
```

Mixer nodes like third-stone's `Mixer4` mostly disappear (levels become
`input * level` expressions or stay as small nodes where smoothing matters).

## 3. Parameter registry & reflection

Endpoint metadata should be enumerable, so GUI, plugin wrapper, and preset
code all consume one table. Oscen is statically compiled, so we get that
property via codegen. For every graph, `graph!` additionally emits:

```rust
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ThirdStoneGraphParam { OscAPitch, OscAPitchKt, /* … */ }

pub struct ParamDescriptor {
    pub id: ThirdStoneGraphParam,
    pub name: &'static str,          // "osc_a_pitch"
    pub default: f32,
    pub range: Option<(f32, f32)>,
    pub center: Option<f32>,         // skew midpoint
    pub unit: Option<&'static str>,
    pub ramp: Option<u32>,
    pub group: Option<&'static str>,
}

impl ThirdStoneGraph {
    pub const PARAMS: &'static [ParamDescriptor];
    pub fn set_param(&mut self, p: ThirdStoneGraphParam, v: f32);   // RT-safe
    pub fn get_param(&self, p: ThirdStoneGraphParam) -> f32;
    pub fn param_by_name(name: &str) -> Option<ThirdStoneGraphParam>; // non-RT ok
}
```

Consequences:

- Metadata moves out of `nih_params` and becomes target-independent;
  `nih_params` *consumes* the descriptor table to build nih-plug params.
  The standalone target stops losing ranges/units.
- Third-stone's hand-written `ParamChange` enum (150 lines), 108-arm match
  (160 lines), and `presets::apply_to_graph` (110 lines) all collapse:
  UI channel = `(Param, f32)`; preset = `&[(Param, f32)]` + 3-line loop.
- This is the key **agent affordance**: tooling can enumerate, validate, and
  set parameters without parsing macro bodies.

Lowest-effort/highest-certainty item: pure codegen addition, no DSL changes.
**Build this first.**

### 3b. Graph-level param conditioning (later)

Third-stone's C15 cp→physical scalings live in a Python preset script
today. A lightweight way to filter/scale a param before it reaches the
graph — inline map expressions on inputs — would bring them into the graph:

```rust
input drive: value = 1.0 [0.0..1.0] |cp| (60.0 * cp * cp);
```

Defer until §1–§3 land; design interacts with descriptor ranges.

## 4. First-class polyphony

Polyphony should be an idiom the system understands. Oscen has the parts
(`MidiParser`, `VoiceAllocator`, `MidiVoiceHandler`) but every project
re-plumbs them. Provide a canned form:

```rust
nodes {
    voices = poly::<8>(ThirdStoneVoice::new());   // allocator+handlers inside
}
connections {
    midi_parser.note_on  -> voices.note_on;
    midi_parser.note_off -> voices.note_off;
    voices.audio_out * 0.3536 -> audio_out;       // §2 makes this legal & explicit
}
```

`poly` shipped (2026-07-08) as a **compiler-desugared construct** rather
than the library-subgraph-template sketched here (superseding the open
question below): a desugar pass expands the statement into the ordinary
allocator/handler/voice-array nodes + connections before lowering, so
everything downstream is unchanged. The original `midi_in -> voices.midi`
sketch is also superseded: **`MidiParser` stays outside poly** — users
wire `midi_parser.note_on -> voices.note_on;` — keeping poly usable for
non-MIDI event sources and parser count explicit. Voice-level endpoints
`frequency`/`gate` are wired internally (default contract, validated
against the voice type's endpoint manifest); everything else hoists
through.

## 5. Host adapter crates (`oscen-nih`, `oscen-standalone`)

The last mile of shipping a synth is host boilerplate. Answer: adapter
crates that own the generic plumbing both of third-stone's targets
hand-wrote:

```rust
// plugin crate
oscen_nih::synth_plugin! {
    graph: ThirdStoneGraph,
    name: "Third Stone", vendor: "Oscen", url: "…",
    clap_id: "com.oscen.thirdstone", vst3_id: b"ThirdStonePlugin",
    features: [Instrument, Synthesizer, Stereo],
}

// standalone
fn main() -> Result<()> {
    oscen_standalone::App::new(ThirdStoneGraph::new())   // cpal + midir + param channel
        .run()
}
```

Centralizes the subtle, easy-to-get-wrong code: MAX_BLOCK_SIZE chunking,
per-chunk event-offset rebasing, NoteEvent→MIDI conversion (velocity
rounding!), audio-thread stack sizing, param-channel drain (typed by §3's
enum). ~350 lines deleted per project; one tested implementation.

`oscen-standalone` should expose the param descriptors (§3) so simple UIs
(egui/slint dev panels) can be auto-generated for quick auditioning.

## 6. Subgraph scheduling transparency

Oscen composes generated structs rather than flattening graphs at compile
time, so subgraphs are scheduled as atomic nodes: third-stone needed ten
spurious `-> [1] ->` edges for connections like `branch_a.env_out ->
osc_a.self_mod_env` where no true cycle exists (the envelope output doesn't
depend on the branch's audio input).

Plan, in two stages:

1. **Diagnostics now**: when a cycle is detected, report the full cycle path
   (node → node → …) and suggest `-> [N] ->`. Today's failure mode costs
   every new user/agent a discovery session.
2. **Per-output dependency masks**: subgraph codegen records, for each
   output, which inputs it transitively depends on (it has the internal
   topology). The parent scheduler uses the mask instead of assuming
   all-inputs→all-outputs; where needed, split the subgraph's `process`
   into phases. The ten hacky edges then compile with zero delay and no
   annotation. High effort; benchmark codegen impact.

## 7. Metadata & event polish

- **Time-based ramps**: `ramp: 50ms` resolved at `init(sample_rate)`, plus a
  graph-level `default ramp: 50ms;`. Third-stone repeats `[ramp: 2205]`
  ("50 ms at 44.1k", undocumented) 80+ times and it's wrong at 48k.
- **Endpoint grouping braces**: `input value { a = 0.0; b = 1.0; }`
  — cosmetic, but real with 107 inputs.
- **Typed note events over raw MIDI**: promote `NoteOn`/`NoteOff` typed
  events in the public API, and generate on any graph with a designated
  MIDI event input:

  ```rust
  graph.push_midi([0x90, note, vel], frame_offset);  // uses EventPayload::Midi — no alloc
  ```

  This closes the discoverability gap that led third-stone to allocate
  `Arc<RawMidiMessage>` on the audio thread. Also: extend
  `realtime_safety.rs` to cover event push paths.

## 8. Tooling & agent affordances

- ~~**Project template**~~: dropped by decision (2026-07-07) — not needed;
  the cookbook plus `poly`/adapter crates cover the on-ramp.
- **Graph introspection dump**: `OSCEN_DUMP_GRAPH=1` (or an xtask) → DOT/JSON
  of nodes, edges, rates, delay-breakers, hoist expansions. Replaces
  hand-maintained ASCII diagrams; lets agents verify topology after edits.
- **Test harness** (`oscen::test` or a `oscen-test` crate): init graph, send
  note events, render N seconds, per-window RMS — third-stone's
  `release_tail.rs` is 80% reusable scaffolding.
- **Cookbook / llms.txt** documenting the idioms third-stone discovered
  empirically: chunked processing, delay-breaker rules, `EventPayload::Midi`
  vs `Object`, why `arrayvec`/`paste` must remain as deps of downstream
  crates, stack sizing for large graphs, voice-sum semantics.

---

## Status (updated as work lands)

- **Phase 1: shipped.** Param registry (`{Graph}Param`, `param_descriptors()`,
  `set_param`/`get_param`), `push_<event_input>()` helpers with
  allocation-free `From<[u8;3]>`/`From<f32>` payload conversions, cycle
  diagnostics that name the cycle path, `docs/COOKBOOK.md`.
- **Phase 2: core shipped.** Hoisted inputs — single
  (`input osc.frequency;`), rename (`input osc.amplitude level = 0.5 [spec];`),
  endpoint lists with `*`-substitution rename patterns
  (`input branch_a.{env_attack, env_decay} a_*;`). Defaults inherited from
  the child at construction (`ReadValueEndpoint`); array hoists broadcast;
  nested-graph ramped inputs driven via `ConnectEndpoints<f32,
  ValueRampState>`. Comma fan-out (`src -> d1, d2;`). Fan-in summing
  already existed. Topo sort made deterministic (declaration-order seeds).
- **Wildcard hoists (`input voices.*;`): shipped (2026-07-07).** Built on
  the manifest-macro CPS mechanism from the spike below. Every
  `#[derive(Node)]` type and every generated `graph!` type exports an
  endpoint manifest (`__oscen_endpoints_<TypeName>!`, pub endpoints only,
  names + kinds, no defaults — defaults still inherit from the constructed
  child at runtime). `graph!` bodies without wildcards compile exactly as
  before; with wildcards, the macro chains the child manifests through the
  hidden `__oscen_graph_resume` proc macro and re-enters compilation with
  the collected endpoint sets, expanding each wildcard into the existing
  single-endpoint hoist machinery. Expansion order = manifest declaration
  order; skips connection-destination endpoints, explicitly hoisted
  endpoints, and `asset` inputs; collisions with other declarations are
  hard errors spanned to the `input node.*;` statement (per the hygiene
  decision below — `except {}` remains the future escape hatch). Nested
  `graph!` types hoist through (the `poly` prerequisite). Cookbook
  documents the manifest-in-scope and same-name-type gotchas.
- **§4 `poly`: shipped (2026-07-08).** Compiler-desugared construct (not a
  library subgraph — the open question below is settled): `voices =
  poly::<N>(Voice::new());` expands before lowering into the hand-wired
  `VoiceAllocator::<N>` + `[MidiVoiceHandler; N]` + voice-array pattern,
  with `voices.note_on`/`voices.note_off` aliasing the allocator and every
  other `voices.<ep>` resolving to the array node (broadcast in, summed
  out). The default voice contract (`frequency`/`gate` inputs, checked
  against the voice type's manifest through the same CPS resolve/resume
  chain as wildcards) is a hard error when unmet, spanned to the `poly`
  statement, suggesting manual wiring as the fallback. `input voices.*;`
  works through it; the contract connections are skip-connected. fm-synth
  (plugin + standalone graphs) ported. **Deferred from §4:** custom
  contract mappings (drum/MPE voices — constraint: overridable mapping,
  no redesign), pluggable allocation policy (v1 keeps LRU-with-release-
  preference), and `voices.midi` sugar (superseded by explicit
  `midi_parser.note_on -> voices.note_on` wiring; a parser-inside variant
  could return later as opt-in sugar).
- **Deferred from Phase 2:** implicit-endpoint chaining (`a -> b -> c`) —
  can now reuse the manifest mechanism if pursued.
- **Not started:** hoisted *outputs*, `except {}` exclusions (now useful:
  wildcards exist and collisions/manual wiring are the motivating cases).
- **Hoist-through spike (2026-07-07): manifest-macro CPS validated.**
  Each `derive(Node)`/`graph!` additionally emits an exported
  `__oscen_endpoints_<Type>!` macro_rules "manifest" carrying the type's
  endpoint list (name, kind, default). A parent macro that needs a child's
  endpoints expands to a manifest invocation with a continuation +
  passthrough state (continuation-passing style). Proven in a 3-crate
  workspace: (1) cross-crate manifest → macro_rules continuation generates
  hoisted fields with child defaults; (2) two manifests chain (multiple
  wildcard nodes per graph); (3) final continuation can be a **proc macro**
  (`$callback:path => (…)`), so the graph compiler can re-enter itself for
  final codegen after collecting all manifests. Known constraints:
  the child type must be named by path (no aliases/generics through the
  manifest lookup); manifest name is derived from the type name, so two
  same-named types from different crates need path-qualified manifest
  resolution; spans on manifest-supplied tokens point at the child crate
  (acceptable — hoist diagnostics should re-span to the parent's `input
  node.*;` statement).

## Phasing

| Phase | Items | Rationale |
|---|---|---|
| **1** | §3 param registry · §7 push_midi/typed events · §6.1 cycle diagnostics · §8 cookbook | Pure additions, no DSL changes, immediate downstream payoff |
| **2** | §1 hoisted endpoints (single, then wildcard, then rename patterns) · §2 fan-in/chaining | The big DSL work; snapshot + parse-recovery tests throughout |
| **3** | §4 `poly` · §5 adapter crates | Builds on 1+2; mostly library/crate work (template dropped) |
| **4** | §6.2 dependency masks · §3b param conditioning · §7 time ramps | Deeper compiler changes, schedule after 2 stabilizes |

Estimated effect on third-stone: ~60% total line reduction; "add a
parameter" drops from ~10 edit sites in 4+ files to one `input` decl, one
optional metadata override, one UI knob.

## Phase 3 design constraints (genericity)

Third-stone is the stress test, not the spec. Nothing in Phase 3 may bake
in instrument-shaped assumptions:

- **`poly` voice contract, not convention.** Wiring to voice
  `frequency`/`gate` inputs is the *default* mapping, expressed through a
  small overridable contract (trait or declared endpoint mapping) — drum
  voices (note+velocity), MPE voices (per-note bend/pressure) must be
  expressible later without redesign.
- **`poly` allocation policy is pluggable** (oldest / quietest /
  round-robin), not hardcoded.
- **`poly` output shape is whatever the voice exposes** — mono, stereo, or
  multiple named stream outputs are summed per-endpoint; all other
  endpoints hoist through untouched. No "one mono audio_out" assumption.
- **`poly` is optional sugar.** The hand-wired
  parser/allocator/handlers path remains fully supported; anything `poly`
  can't express falls back to explicit wiring.
- **Adapter crates cover effects, not just synths.** One
  `oscen_nih::plugin!` with declared I/O (audio in/out ports, optional MIDI
  in); `synth_plugin!`/`effect_plugin!` at most thin presets over it.
  Channel counts derive from the graph's stream endpoints, never hardcoded
  stereo. `oscen-standalone` likewise: audio input (or file playback) for
  effects, MIDI optional.
- **Acceptance criteria:** fm-synth (instrument), third-stone (large
  instrument), *and* nih-twin-peaks (mono effect, no MIDI) all port onto
  the new machinery with less code and zero special cases. An escape hatch
  needed by any of them is a design smell to fix, not paper over.

## Open questions

- Hoist syntax: `input voices.*;` vs `forward voices.*;` — reusing `input`
  reads well, but complicates parse recovery (an ident path where a name
  was expected). Decide during Phase 2 spike.
- Wildcard hygiene: should `input voices.*` fail or warn when a child adds
  an endpoint that collides with an existing parent input? (Proposal: hard
  error, with `except` as the escape hatch.)
- Descriptor stability: is `ThirdStoneGraphParam` ordinal order a public
  contract (preset serialization)? Proposal: order = declaration order,
  documented as stable; name-based lookup for durable storage.
- ~~`poly`: template subgraph in `oscen-lib` vs compiler-known construct.~~
  Settled (2026-07-08): compiler-desugared construct. A library subgraph
  couldn't be generic over the voice type (graphs are static structs, no
  type parameters), while the desugar reuses the manifest machinery and
  produces ordinary nodes — see §4 and the Status entry.
- Does `nih_params` remain a graph-item, or move to the `oscen_nih` macro
  entirely once descriptors exist? (Cleaner layering if it moves.)
