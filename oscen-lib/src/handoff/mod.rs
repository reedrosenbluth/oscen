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
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Fixed capacity of the return ring. The SPSC protocol keeps at most one retired
/// value outstanding between producer drains, so a small constant is ample.
const RETURN_CAPACITY: usize = 8;

/// State shared by the two halves of a handoff.
///
/// `pending` is a cheap "something to take" hint. `ArcSwapOption::swap` is
/// not a bare pointer exchange: arc-swap waits for in-flight readers by
/// walking its global debt list on every swap, and claims a per-thread
/// bookkeeping node (a heap allocation) the first time a given thread
/// swaps. Consumers poll from per-sample `process()` paths, so the idle
/// path must be a single atomic load that never reaches arc-swap at all.
struct Shared<T> {
    slot: ArcSwapOption<T>,
    pending: AtomicBool,
}

/// Create a connected publisher/consumer pair sharing one handoff slot.
pub fn pair<T: Send>() -> (Publisher<T>, Consumer<T>) {
    let shared = Arc::new(Shared {
        slot: ArcSwapOption::<T>::empty(),
        pending: AtomicBool::new(false),
    });
    // The return path flows audio→producer, so it is the reverse of the forward
    // direction: the audio side owns the `rtrb::Producer` and the non-RT side
    // owns the `rtrb::Consumer`.
    let (returns_tx, returns_rx) = rtrb::RingBuffer::new(RETURN_CAPACITY);
    (
        Publisher {
            shared: shared.clone(),
            returns_rx,
        },
        Consumer { shared, returns_tx },
    )
}

/// Non-realtime producer side. May allocate and block.
pub struct Publisher<T: Send> {
    shared: Arc<Shared<T>>,
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
        let displaced = self.shared.slot.swap(Some(Arc::new(value)));
        drop(displaced);
        // Publish the hint after the slot holds the value: a consumer that
        // observes `pending == true` (Acquire) is guaranteed to find it.
        self.shared.pending.store(true, Ordering::Release);
        // Drain the return ring, dropping every reclaimed value off-thread.
        while let Ok(retired) = self.returns_rx.pop() {
            drop(retired);
        }
    }
}

/// Realtime consumer side. Lock-, block-, and drop-free; see [`Consumer::take`]
/// for the one allocation caveat.
pub struct Consumer<T: Send> {
    shared: Arc<Shared<T>>,
    returns_tx: rtrb::Producer<Arc<T>>,
}

impl<T: Send> Consumer<T> {
    /// Returns the newest published value at most once per `publish`,
    /// otherwise `None`. Never locks, blocks, or drops.
    ///
    /// **Idle path** (nothing published since the last take): one atomic
    /// load, nothing else. This is what a per-sample poll costs almost all
    /// of the time.
    ///
    /// **Pending path**: one `arc-swap` swap, which also runs arc-swap's
    /// reader-debt bookkeeping and, the *first* time a given thread ever
    /// swaps, may allocate arc-swap's per-thread node. That one-time
    /// allocation happens on the first take *of a published value* on the
    /// audio thread; an idle take on a fresh thread does not allocate.
    ///
    /// A `publish` that lands between the flag reset and the swap simply
    /// leaves the flag set again; the following `take` then swaps `None` out
    /// harmlessly. Values are never lost or delivered twice.
    pub fn take(&mut self) -> Option<Arc<T>> {
        if !self.shared.pending.load(Ordering::Acquire) {
            return None;
        }
        // Reset the hint *before* swapping so a publish that races with the
        // swap re-arms it; resetting afterwards could strand a value.
        self.shared.pending.store(false, Ordering::SeqCst);
        self.shared.slot.swap(None)
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
