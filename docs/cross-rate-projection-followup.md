# `CrossRateKernel` projection — TERMINAL STATE

`CrossRateKernel` is a type-level state-shape registry consumed by the
`graph!` macro for two purposes:

1. **`::State` projection.** For `(StreamKind, StreamKind)` edges, codegen
   reads `<() as CrossRateKernel<...>>::State` to choose the
   resampler-state field type (`UpState<K, N>` or `DownState<K, N>`).
2. **Const-time kind-tuple validation.** For every cross-rate edge whose
   source AND destination are projectable, codegen emits a
   `quote_spanned!`-spanned const block that requires
   `(): CrossRateKernel<<#src as EndpointAt<…>>::Kind, <#dst as
   EndpointAt<…>>::Kind, Policy, N, Dir>`. Unsupported tuples fail trait
   resolution; `#[diagnostic::on_unimplemented(...)]` formats the message
   and `quote_spanned!`'s span attribution puts the diagnostic at the
   user's connection token.

There are no behavioral methods on the trait. Lifecycle ordering is owned
by the macro's codegen and performed by direct calls to the concrete
`StreamUpsampler` / `StreamDownsampler` traits against `state.kernel`.

See `docs/superpowers/specs/2026-05-06-dispatch-trait-typetable-collapse-design.md`
for the rationale.

## What the trait is

```rust
#[diagnostic::on_unimplemented(
    message = "no cross-rate kernel for {SrcKind} -> {DstKind} with policy {Policy}",
    note = "valid kind pairs are: (StreamKind, StreamKind), (ValueKind, ValueKind), (ValueKind, StreamKind), (EventKind, EventKind)",
    label = "edge has no resampler"
)]
pub trait CrossRateKernel<SrcKind, DstKind, Policy, const N: u32, Dir> {
    type State: Default + Send;
}
```

Each impl in `dispatch/{stream,value,event}.rs` declares the per-edge
state-shape. `stream.rs` impls use `UpState<K, N>` / `DownState<K, N>`
(read by codegen for stream/stream State projection). `value.rs` and
`event.rs` impls use `type State = ()` — their existence is required for
const-time kind validation, but the State is never queried at runtime
because codegen's kind-gate routes value/event edges through dedicated
fallbacks.

## Where the work lives

- The macro's codegen owns lifecycle ordering: warmup before the inner
  loop, per-edge writes inside the inner loop, finalize after. It calls
  the concrete `StreamUpsampler` / `StreamDownsampler` traits in
  `oscen::resample` directly against `state.kernel`.
- `cross_rate_kernel_state_type` (in `codegen.rs`) projects `<() as
  CrossRateKernel<...>>::State` only for `(Stream, Stream)` edges. Value
  and event cross-rate edges go through `kernel_up_type` /
  `kernel_down_type` (concrete kernel) and dedicated event drains.
- `validate_cross_rate_kinds` (in `rate_analysis.rs`) rejects unsupported
  kind tuples at macro expansion time with a `compile_error!` spanned at
  the offending connection — defense-in-depth for cases where kinds are
  inferable from graph-i/o propagation.
- `generate_kind_assertions` (in `codegen.rs`) emits a `quote_spanned!`
  const-time trait-bound assertion per cross-rate edge with projectable
  endpoints. Drives `on_unimplemented` for the broader case where
  node-to-node `Field` expressions need type-system queries to determine
  kinds.

The supported kind tuples:
`(Stream, Stream)`, `(Value, Value)`, `(Value, Stream)`, `(Event, Event)`.

## What is no longer in scope

The original followup listed the following as deferred work:
"Lifecycle-method emission" and "Same-rate dispatch unification".
**Lifecycle-method emission is rejected** — the trait is type-only by
design. Same-rate dispatch unification is independent of this trait and
tracked elsewhere if pursued.

## Known limitation

Compound source expressions (e.g., `(osc.output * 2.0) -> filter.input`
across rates) have no single `EndpointAt`-projectable field on the
source side; the const-assertion is skipped for these edges. They
continue to fall through to `ConnectEndpoints` on type errors. This is
a narrower miss than today.

## Consumer-crate requirement

Consumer crates still need `#![feature(inherent_associated_types)]` at
the crate root for `<NodeType>::field__Ep` to resolve. That requirement
is unchanged by this collapse and is tracked separately as a possible
stable-encoding spike.
