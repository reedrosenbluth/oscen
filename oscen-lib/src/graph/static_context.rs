use arrayvec::ArrayVec;

use super::traits::EventContext;
use super::types::EventInstance;
use super::types::EventPayload;

/// Maximum number of pending events that can be emitted during a single process() call
/// across all nodes in a static graph.
const MAX_PENDING_EVENTS: usize = 64;

/// Pending event with its output endpoint index and optional array index
#[derive(Debug, Clone)]
pub struct PendingEvent {
    pub output_index: usize,
    pub array_index: Option<usize>,
    pub event: EventInstance,
}

/// Lightweight context for static graphs that only supports event emission.
///
/// Unlike ProcessingContext used in runtime graphs, StaticContext is minimal:
/// - No stream/value input slices (static graphs use direct field access)
/// - Uses stack-allocated ArrayVec instead of heap-allocated Vec
/// - Zero overhead in release builds when inlined
pub struct StaticContext<'a> {
    pending_events: &'a mut ArrayVec<PendingEvent, MAX_PENDING_EVENTS>,
}

impl<'a> StaticContext<'a> {
    /// Create a new StaticContext with a mutable reference to the pending events queue
    #[inline]
    pub fn new(pending_events: &'a mut ArrayVec<PendingEvent, MAX_PENDING_EVENTS>) -> Self {
        Self { pending_events }
    }

    /// Emit an event from an output endpoint.
    ///
    /// # Panics
    /// Panics in debug builds if the pending events queue is full.
    /// In release builds, silently drops the event if queue is full.
    #[inline]
    pub fn emit_event(&mut self, output_index: usize, event: EventInstance) {
        let pending = PendingEvent {
            output_index,
            array_index: None,
            event,
        };

        if self.pending_events.try_push(pending).is_err() {
            #[cfg(debug_assertions)]
            panic!(
                "Static graph event queue overflow: attempted to emit more than {} events in a single process() call",
                MAX_PENDING_EVENTS
            );

            #[cfg(not(debug_assertions))]
            {
                // In release builds, silently drop the event
                // This prevents audio glitches from panics, but is a potential bug
            }
        }
    }

    /// Emit a timed event with a frame offset and payload.
    #[inline]
    pub fn emit_timed_event(
        &mut self,
        output_index: usize,
        frame_offset: u32,
        payload: EventPayload,
    ) {
        self.emit_event(
            output_index,
            EventInstance {
                frame_offset,
                payload,
            },
        );
    }

    /// Emit a scalar event (convenience method for f32 payloads).
    #[inline]
    pub fn emit_scalar_event(&mut self, output_index: usize, frame_offset: u32, payload: f32) {
        self.emit_timed_event(output_index, frame_offset, EventPayload::scalar(payload));
    }

    /// Emit an event to a specific array index.
    /// For static graphs, this stores the array index so the graph can route it correctly.
    /// The graph codegen will handle routing to the correct array element.
    #[inline]
    pub fn emit_event_to_array(
        &mut self,
        output_index: usize,
        array_index: usize,
        event: EventInstance,
    ) {
        let pending = PendingEvent {
            output_index,
            array_index: Some(array_index),
            event,
        };

        if self.pending_events.try_push(pending).is_err() {
            #[cfg(debug_assertions)]
            panic!(
                "Static graph event queue overflow: attempted to emit more than {} events in a single process() call",
                MAX_PENDING_EVENTS
            );

            #[cfg(not(debug_assertions))]
            {
                // In release builds, silently drop the event
            }
        }
    }
}

/// Implement EventContext for StaticContext
impl<'a> EventContext for StaticContext<'a> {
    #[inline]
    fn emit_event(&mut self, output_index: usize, event: EventInstance) {
        self.emit_event(output_index, event);
    }

    #[inline]
    fn emit_timed_event(
        &mut self,
        output_index: usize,
        frame_offset: u32,
        payload: EventPayload,
    ) {
        self.emit_timed_event(output_index, frame_offset, payload);
    }

    #[inline]
    fn emit_scalar_event(&mut self, output_index: usize, frame_offset: u32, payload: f32) {
        self.emit_scalar_event(output_index, frame_offset, payload);
    }

    #[inline]
    fn emit_event_to_array(
        &mut self,
        output_index: usize,
        array_index: usize,
        event: EventInstance,
    ) {
        self.emit_event_to_array(output_index, array_index, event);
    }
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_emit_scalar_event() {
        let mut pending = ArrayVec::new();
        let mut ctx = StaticContext::new(&mut pending);

        ctx.emit_scalar_event(0, 10, 440.0);

        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].output_index, 0);
        assert_eq!(pending[0].event.frame_offset, 10);
        assert_eq!(pending[0].event.payload.as_scalar(), Some(440.0));
    }

    #[test]
    fn test_emit_multiple_events() {
        let mut pending = ArrayVec::new();
        let mut ctx = StaticContext::new(&mut pending);

        for i in 0..10 {
            ctx.emit_scalar_event(i, i as u32, i as f32);
        }

        assert_eq!(pending.len(), 10);
        for i in 0..10 {
            assert_eq!(pending[i].output_index, i);
            assert_eq!(pending[i].event.frame_offset, i as u32);
            assert_eq!(pending[i].event.payload.as_scalar(), Some(i as f32));
        }
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "Static graph event queue overflow")]
    fn test_queue_overflow_panics_in_debug() {
        let mut pending = ArrayVec::new();
        let mut ctx = StaticContext::new(&mut pending);

        // Attempt to emit more than MAX_PENDING_EVENTS
        for i in 0..=MAX_PENDING_EVENTS {
            ctx.emit_scalar_event(0, 0, i as f32);
        }
    }

    #[test]
    #[cfg(not(debug_assertions))]
    fn test_queue_overflow_silent_in_release() {
        let mut pending = ArrayVec::new();
        let mut ctx = StaticContext::new(&mut pending);

        // Attempt to emit more than MAX_PENDING_EVENTS (should not panic)
        for i in 0..=MAX_PENDING_EVENTS {
            ctx.emit_scalar_event(0, 0, i as f32);
        }

        // Should have capped at MAX_PENDING_EVENTS
        assert_eq!(pending.len(), MAX_PENDING_EVENTS);
    }
}
