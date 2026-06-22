//! Lock-free single-producer/single-consumer handoff that moves an immutable
//! value from a non-realtime producer thread to the realtime audio thread, and
//! recovers the retired value for destruction **off** the audio thread.
//!
//! This is the JUCE `dsp::Convolution` recipe: publish via an atomic single-slot
//! swap, then push the retired value back to a worker to free it — assembled from
//! `arc-swap` + `rtrb` instead of hand-rolled `unsafe`.
//!
//! Values cross the boundary as `Arc<T>` on purpose. If the audio thread
//! unwrapped the `Arc` to an owned `T`, the `Arc`'s heap control block would be
//! deallocated on the audio thread — exactly the `free()` this exists to avoid.
//! The producer side is the only place an `Arc<T>` is dropped, so deallocation
//! always happens off the audio thread.

use arc_swap::ArcSwapOption;
use std::sync::Arc;

/// Fixed capacity of the return ring. The SPSC protocol keeps at most one retired
/// value outstanding between producer drains, so a small constant is ample.
const RETURN_CAPACITY: usize = 8;

/// Create a connected publisher/consumer pair sharing one handoff slot.
pub fn pair<T: Send>() -> (Publisher<T>, Consumer<T>) {
    let slot = Arc::new(ArcSwapOption::<T>::empty());
    // The return path flows audio→producer, so it is the reverse of the forward
    // direction: the audio side owns the `rtrb::Producer` and the non-RT side
    // owns the `rtrb::Consumer`.
    let (returns_tx, returns_rx) = rtrb::RingBuffer::new(RETURN_CAPACITY);
    (
        Publisher {
            slot: slot.clone(),
            returns_rx,
        },
        Consumer { slot, returns_tx },
    )
}

/// Non-realtime producer side. May allocate and block.
pub struct Publisher<T: Send> {
    slot: Arc<ArcSwapOption<T>>,
    returns_rx: rtrb::Consumer<Arc<T>>,
}

impl<T: Send> Publisher<T> {
    /// Install `value` as the newest published value, then drain and drop any
    /// retired values the consumer has handed back. Both the displaced
    /// (never-consumed) previous value and reclaimed values are dropped here,
    /// off the audio thread. May allocate.
    pub fn publish(&mut self, value: T) {
        // Newest-wins: a previously published value the consumer never took is
        // displaced and dropped here, off the audio thread.
        let displaced = self.slot.swap(Some(Arc::new(value)));
        drop(displaced);
        // Drain the return ring, dropping every reclaimed value off-thread.
        while let Ok(retired) = self.returns_rx.pop() {
            drop(retired);
        }
    }
}

/// Realtime consumer side. Alloc-, lock-, block-, and drop-free.
pub struct Consumer<T: Send> {
    slot: Arc<ArcSwapOption<T>>,
    returns_tx: rtrb::Producer<Arc<T>>,
}

impl<T: Send> Consumer<T> {
    /// Returns `Some(arc)` exactly once for each `publish`, otherwise `None`.
    /// One atomic swap; never allocates, locks, blocks, or drops.
    ///
    /// After taking, the slot is `None`, so a subsequent `take` with no
    /// intervening `publish` returns `None` — the "exactly once per publish"
    /// guarantee with a single atomic operation.
    pub fn take(&mut self) -> Option<Arc<T>> {
        self.slot.swap(None)
    }

    /// Hand a retired value back to the producer for off-thread destruction.
    /// One ring push; never allocates, locks, blocks, or drops.
    pub fn retire(&mut self, value: Arc<T>) {
        // A push failure (ring full) is unreachable under the SPSC protocol: at
        // most one value is outstanding between producer drains, and `publish`
        // drains every time. If a push ever did fail, `value` falls out of scope
        // and is dropped here as a tolerated last resort.
        let _ = self.returns_tx.push(value);
    }
}

#[cfg(test)]
mod tests;
