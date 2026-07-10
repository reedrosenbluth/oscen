// ============================================================================
// Trait-Based Connection Dispatch
// ============================================================================

/// Trait for connecting two endpoints in a static graph.
///
/// This trait enables compile-time dispatch for different endpoint type combinations.
/// The Rust compiler selects the appropriate implementation based on the actual field
/// types, eliminating the need for the macro to know endpoint types at expansion time.
///
/// # Example
/// ```ignore
/// // Macro generates generic code:
/// <() as ConnectEndpoints<_, _>>::connect(
///     &self.source.gate,
///     &mut self.dest.gate
/// );
///
/// // Compiler selects the right impl based on actual types:
/// // - If both are StaticEventQueue, use event→event impl
/// // - If both are f32 (or any Copy payload), use the copy impl
/// ```
///
/// Stream/value/event *kind* compatibility is enforced separately, via the
/// `EndpointAt::Kind` markers checked by the graph macro's edge assertions —
/// this trait only moves payloads between already-validated endpoints.
#[diagnostic::on_unimplemented(
    message = "no connection from {Src} to {Dst}",
    note = "supported: matching payloads (ValuePayload types like f32, Frame<N>, arrays), EventOutput -> EventInput/StaticEventQueue",
    label = "incompatible endpoint pair"
)]
pub trait ConnectEndpoints<Src, Dst> {
    fn connect(src: &Src, dst: &mut Dst);
}

// Matching plain payloads: any `ValuePayload` (f32 and other Copy value
// types), Frame<C> → Frame<C>, and arrays of each. Covers node-to-node
// edges, graph inputs, and graph outputs alike, since plain endpoint fields
// and graph buffers share the same types.
//
// The `ValuePayload` blanket is safe where a blanket `T: Copy` impl would
// not be: `ValuePayload` is a local opt-in marker, so coherence can prove
// the event-queue and reference impls below disjoint from it (none of those
// types implement — or can implement — `ValuePayload`). `Frame<C>` stays a
// concrete impl and is deliberately *not* a `ValuePayload`: making it one
// would overlap this blanket.
impl<T: super::types::ValuePayload> ConnectEndpoints<T, T> for () {
    #[inline]
    fn connect(src: &T, dst: &mut T) {
        *dst = *src;
    }
}

impl<const C: usize> ConnectEndpoints<crate::frame::Frame<C>, crate::frame::Frame<C>> for () {
    #[inline]
    fn connect(src: &crate::frame::Frame<C>, dst: &mut crate::frame::Frame<C>) {
        *dst = *src;
    }
}

impl<const N: usize> ConnectEndpoints<[f32; N], [f32; N]> for () {
    #[inline]
    fn connect(src: &[f32; N], dst: &mut [f32; N]) {
        dst.copy_from_slice(src);
    }
}

impl<const C: usize, const N: usize>
    ConnectEndpoints<[crate::frame::Frame<C>; N], [crate::frame::Frame<C>; N]> for ()
{
    #[inline]
    fn connect(src: &[crate::frame::Frame<C>; N], dst: &mut [crate::frame::Frame<C>; N]) {
        dst.copy_from_slice(src);
    }
}

// Reference → value (for summing arrays)
impl<T: Copy> ConnectEndpoints<&T, T> for () {
    #[inline]
    fn connect(src: &&T, dst: &mut T) {
        *dst = **src;
    }
}

// f32 → ramped value input (hoisting a nested graph's ramped input, or any
// per-frame value edge into a `ValueRampState` field). The write is a
// per-frame stream of already-conditioned values (the parent's own ramp
// smooths setter calls), so the destination follows exactly rather than
// re-ramping — a ramp on top of a ramp would double the lag.
impl ConnectEndpoints<f32, super::types::ValueRampState> for () {
    #[inline]
    fn connect(src: &f32, dst: &mut super::types::ValueRampState) {
        dst.set_immediate(*src);
    }
}

/// Read the effective payload of a value endpoint regardless of its storage:
/// a plain payload field (`f32` or any other [`ValuePayload`]) or a ramped
/// `ValueRampState` (which reads as its current `f32`). Used by generated
/// code to inherit a hoisted input's initial value from the child node it
/// hoists (`input voices.cutoff;` with no `= default`), where the macro
/// cannot know the child field's concrete type at expansion time.
///
/// [`ValuePayload`]: super::types::ValuePayload
pub trait ReadValueEndpoint {
    type Value: super::types::ValuePayload;
    fn read_value(&self) -> Self::Value;
}

impl<T: super::types::ValuePayload> ReadValueEndpoint for T {
    type Value = T;
    #[inline]
    fn read_value(&self) -> T {
        *self
    }
}

impl ReadValueEndpoint for super::types::ValueRampState {
    type Value = f32;
    #[inline]
    fn read_value(&self) -> f32 {
        self.current
    }
}

// Event → Event (StaticEventQueue to StaticEventQueue)
impl ConnectEndpoints<super::types::StaticEventQueue, super::types::StaticEventQueue> for () {
    #[inline]
    fn connect(src: &super::types::StaticEventQueue, dst: &mut super::types::StaticEventQueue) {
        dst.clear();
        // Copy all events from source to destination
        for event in src.iter() {
            super::types::debug_assert_event_pushed(dst.try_push(event.clone()));
        }
    }
}

// EventOutput → EventInput (direct node-to-node event routing)
// This implementation enables trait-based dispatch for event connections
// without requiring the macro to know endpoint types at expansion time.
impl<S, D> ConnectEndpoints<super::types::EventOutput<S>, super::types::EventInput<D>> for () {
    #[inline]
    fn connect(src: &super::types::EventOutput<S>, dst: &mut super::types::EventInput<D>) {
        dst.clear();
        // Copy all events from source output to destination input
        for event in src.iter() {
            super::types::debug_assert_event_pushed(dst.try_push(event.clone()));
        }
    }
}

// EventInput → EventInput (for graph-level event input forwarding)
impl<S, D> ConnectEndpoints<super::types::EventInput<S>, super::types::EventInput<D>> for () {
    #[inline]
    fn connect(src: &super::types::EventInput<S>, dst: &mut super::types::EventInput<D>) {
        dst.clear();
        // Copy all events from source to destination
        for event in src.iter() {
            super::types::debug_assert_event_pushed(dst.try_push(event.clone()));
        }
    }
}

// EventOutput array → EventInput array (for polyphonic voice routing)
impl<S, D, const N: usize>
    ConnectEndpoints<[super::types::EventOutput<S>; N], [super::types::EventInput<D>; N]> for ()
{
    #[inline]
    fn connect(
        src: &[super::types::EventOutput<S>; N],
        dst: &mut [super::types::EventInput<D>; N],
    ) {
        for (s, d) in src.iter().zip(dst.iter_mut()) {
            d.clear();
            for event in s.iter() {
                super::types::debug_assert_event_pushed(d.try_push(event.clone()));
            }
        }
    }
}

// StaticEventQueue → EventInput (graph input → node input)
impl<T> ConnectEndpoints<super::types::StaticEventQueue, super::types::EventInput<T>> for () {
    #[inline]
    fn connect(src: &super::types::StaticEventQueue, dst: &mut super::types::EventInput<T>) {
        dst.clear();
        for event in src.iter() {
            super::types::debug_assert_event_pushed(dst.try_push(event.clone()));
        }
    }
}

// EventOutput → StaticEventQueue (node output → graph output)
impl<T> ConnectEndpoints<super::types::EventOutput<T>, super::types::StaticEventQueue> for () {
    #[inline]
    fn connect(src: &super::types::EventOutput<T>, dst: &mut super::types::StaticEventQueue) {
        dst.clear();
        for event in src.iter() {
            super::types::debug_assert_event_pushed(dst.try_push(event.clone()));
        }
    }
}

// ============================================================================
// Fan-in accumulation dispatch
// ============================================================================

/// Accumulate one more source into a destination already initialized by a
/// [`ConnectEndpoints::connect`] call. Used to lower **stream fan-in**: when ≥2
/// stream sources connect to one destination, the graph macro emits a single
/// `connect` for the first source followed by one `accumulate` per remaining
/// source, so the destination ends up holding the **sum** of its sources.
///
/// The dispatch mirrors [`ConnectEndpoints`]: coherence selects the impl from
/// the actual field types. Stream payloads (`f32`, `Frame<N>`) sum; event
/// endpoints append their events to the destination queue (already initialized
/// by the first source's `connect`), so an event fan-in merges all sources.
#[diagnostic::on_unimplemented(
    message = "no fan-in accumulation from {Src} into {Dst}",
    note = "fan-in summing supports matching stream payloads (f32, Frame<N>); \
            event endpoints append their events",
    label = "endpoint pair cannot be summed"
)]
pub trait AccumulateEndpoints<Src, Dst> {
    fn accumulate(src: &Src, dst: &mut Dst);
}

// Stream payloads sum element-wise (`f32` and `Frame<N>` both implement `Add`).
impl AccumulateEndpoints<f32, f32> for () {
    #[inline]
    fn accumulate(src: &f32, dst: &mut f32) {
        *dst += *src;
    }
}

impl<const C: usize> AccumulateEndpoints<crate::frame::Frame<C>, crate::frame::Frame<C>> for () {
    #[inline]
    fn accumulate(src: &crate::frame::Frame<C>, dst: &mut crate::frame::Frame<C>) {
        *dst = *dst + *src;
    }
}

// Event endpoints have no summation; a multi-source event fan-in appends each
// extra source's events to the destination queue. The first source's `connect`
// already cleared and initialized the destination, so appending (rather than
// delegating to `connect`, which clears) preserves events from every source.
impl<S, D> AccumulateEndpoints<super::types::EventOutput<S>, super::types::EventInput<D>> for () {
    #[inline]
    fn accumulate(src: &super::types::EventOutput<S>, dst: &mut super::types::EventInput<D>) {
        for event in src.iter() {
            super::types::debug_assert_event_pushed(dst.try_push(event.clone()));
        }
    }
}

impl<S, D> AccumulateEndpoints<super::types::EventInput<S>, super::types::EventInput<D>> for () {
    #[inline]
    fn accumulate(src: &super::types::EventInput<S>, dst: &mut super::types::EventInput<D>) {
        for event in src.iter() {
            super::types::debug_assert_event_pushed(dst.try_push(event.clone()));
        }
    }
}

impl<D> AccumulateEndpoints<super::types::StaticEventQueue, super::types::EventInput<D>> for () {
    #[inline]
    fn accumulate(src: &super::types::StaticEventQueue, dst: &mut super::types::EventInput<D>) {
        for event in src.iter() {
            super::types::debug_assert_event_pushed(dst.try_push(event.clone()));
        }
    }
}
