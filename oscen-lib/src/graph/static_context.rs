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
/// // - If both are f32, use value→value impl
/// ```
pub trait ConnectEndpoints<Src, Dst> {
    fn connect(src: &Src, dst: &mut Dst);
}

// Event → Event (StaticEventQueue to StaticEventQueue)
impl ConnectEndpoints<super::types::StaticEventQueue, super::types::StaticEventQueue> for () {
    #[inline]
    fn connect(
        src: &super::types::StaticEventQueue,
        dst: &mut super::types::StaticEventQueue,
    ) {
        dst.clear();
        // Copy all events from source to destination
        for event in src.iter() {
            let _ = dst.try_push(event.clone());
        }
    }
}

// Value → Value (f32 to f32)
// This handles both value inputs and stream connections (both are f32)
impl ConnectEndpoints<f32, f32> for () {
    #[inline]
    fn connect(src: &f32, dst: &mut f32) {
        *dst = *src;
    }
}

// Reference → Value (for summing arrays)
impl ConnectEndpoints<&f32, f32> for () {
    #[inline]
    fn connect(src: &&f32, dst: &mut f32) {
        *dst = **src;
    }
}

// Array → Array (fixed-size arrays, like [f32; 32])
impl<const N: usize> ConnectEndpoints<[f32; N], [f32; N]> for () {
    #[inline]
    fn connect(src: &[f32; N], dst: &mut [f32; N]) {
        dst.copy_from_slice(src);
    }
}

// EventOutput → EventInput (direct node-to-node event routing)
// This implementation enables trait-based dispatch for event connections
// without requiring the macro to know endpoint types at expansion time.
impl<S, D> ConnectEndpoints<super::types::EventOutput<S>, super::types::EventInput<D>> for () {
    #[inline]
    fn connect(
        src: &super::types::EventOutput<S>,
        dst: &mut super::types::EventInput<D>,
    ) {
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
    fn connect(
        src: &super::types::EventInput<S>,
        dst: &mut super::types::EventInput<D>,
    ) {
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
    fn connect(
        src: &super::types::StaticEventQueue,
        dst: &mut super::types::EventInput<T>,
    ) {
        dst.clear();
        for event in src.iter() {
            let _ = dst.try_push(event.clone());
        }
    }
}

// EventOutput → StaticEventQueue (node output → graph output)
impl<T> ConnectEndpoints<super::types::EventOutput<T>, super::types::StaticEventQueue> for () {
    #[inline]
    fn connect(
        src: &super::types::EventOutput<T>,
        dst: &mut super::types::StaticEventQueue,
    ) {
        dst.clear();
        for event in src.iter() {
            let _ = dst.try_push(event.clone());
        }
    }
}
