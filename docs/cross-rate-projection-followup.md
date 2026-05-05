# `CrossRateKernel` projection — RESOLVED

Activated in commits `28da1af`..`c2748b8` (this branch's `multirate-graph` work):

- `derive(Node)` emits per-endpoint markers as inherent associated types
  reachable via `<NodeType>::field__Ep`. (`oscen-macros/src/lib.rs`)
- `graph!` codegen drops the multi-segment guard and projects through these
  aliases. (`oscen-macros/src/graph_macro/codegen.rs::endpoint_marker_tokens`)
- Projection is kind-gated to stream/stream edges; value and event cross-rate
  edges keep the concrete-kernel fallback (`LatchUp`/`LatchDown`, dedicated
  event drains). This avoids a `.kernel` drill-through compile error on State
  shapes without that field, without requiring the lifecycle-method refactor.
- `oscen-lib/tests/multirate_graph.rs::value_cross_rate_latches_across_inner_ticks`
  covers the value path; `projection_fires_on_bare_ident_node_type` covers the
  projection path.

Out of scope (still tracked):
- Lifecycle-method emission (`before_inner` / `on_inner` / `after_inner`).
- Same-rate dispatch unification.
- Eliminating the `voices` field-name match in per-voice loop emission.

Consumer crates require `#![feature(inherent_associated_types)]` at the crate
root. Workspace is nightly-only; this is a one-line addition per consumer.
