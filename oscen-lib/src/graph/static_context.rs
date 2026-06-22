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
    note = "supported: matching Copy payloads (f32, Frame<N>, arrays), EventOutput -> EventInput/ArrayVec<EventInstance, 32>",
    label = "incompatible endpoint pair"
)]
pub trait ConnectEndpoints<Src, Dst> {
    fn connect(src: &Src, dst: &mut Dst);
}

// Matching plain payloads: f32 → f32, Frame<C> → Frame<C>, and arrays of
// each. Covers node-to-node edges, graph inputs, and graph outputs alike,
// since plain endpoint fields and graph buffers share the same types.
// (These are enumerated concretely rather than as a blanket `T: Copy` impl,
// which would overlap the event-queue impls below under coherence rules.)
impl ConnectEndpoints<f32, f32> for () {
    #[inline]
    fn connect(src: &f32, dst: &mut f32) {
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

// Event → Event (StaticEventQueue to StaticEventQueue)
impl ConnectEndpoints<super::types::StaticEventQueue, super::types::StaticEventQueue> for () {
    #[inline]
    fn connect(src: &super::types::StaticEventQueue, dst: &mut super::types::StaticEventQueue) {
        dst.clear();
        // Copy all events from source to destination
        for event in src.iter() {
            let _ = dst.try_push(event.clone());
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
            let _ = dst.try_push(event.clone());
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
            let _ = dst.try_push(event.clone());
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
                let _ = d.try_push(event.clone());
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
            let _ = dst.try_push(event.clone());
        }
    }
}

// EventOutput → StaticEventQueue (node output → graph output)
impl<T> ConnectEndpoints<super::types::EventOutput<T>, super::types::StaticEventQueue> for () {
    #[inline]
    fn connect(src: &super::types::EventOutput<T>, dst: &mut super::types::StaticEventQueue) {
        dst.clear();
        for event in src.iter() {
            let _ = dst.try_push(event.clone());
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
/// endpoints fall back to plain `connect` (last-write-wins), so an event fan-in
/// keeps its existing behavior and still compiles (event queues have no `Add`).
#[diagnostic::on_unimplemented(
    message = "no fan-in accumulation from {Src} into {Dst}",
    note = "fan-in summing supports matching stream payloads (f32, Frame<N>); \
            event endpoints keep last-write-wins",
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

// Event endpoints have no summation; a multi-source event fan-in keeps the
// existing last-write-wins behavior by delegating to `connect`.
impl<S, D> AccumulateEndpoints<super::types::EventOutput<S>, super::types::EventInput<D>> for () {
    #[inline]
    fn accumulate(src: &super::types::EventOutput<S>, dst: &mut super::types::EventInput<D>) {
        <() as ConnectEndpoints<_, _>>::connect(src, dst);
    }
}

impl<S, D> AccumulateEndpoints<super::types::EventInput<S>, super::types::EventInput<D>> for () {
    #[inline]
    fn accumulate(src: &super::types::EventInput<S>, dst: &mut super::types::EventInput<D>) {
        <() as ConnectEndpoints<_, _>>::connect(src, dst);
    }
}

impl<D> AccumulateEndpoints<super::types::StaticEventQueue, super::types::EventInput<D>> for () {
    #[inline]
    fn accumulate(src: &super::types::StaticEventQueue, dst: &mut super::types::EventInput<D>) {
        <() as ConnectEndpoints<_, _>>::connect(src, dst);
    }
}
