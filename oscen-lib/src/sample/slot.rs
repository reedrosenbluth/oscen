//! Realtime-swappable shared slots and a named registry on top of them.
//!
//! A [`SampleSlot`] is a cloneable handle to one shared, atomically-swappable
//! payload (`Arc<T>` — a [`SampleBuffer`](super::SampleBuffer) for players, or a
//! prepared convolution kernel later). The control thread calls [`SampleSlot::store`]
//! to swap in new data; audio-thread readers cache their own `Arc` and only
//! refresh when the slot's generation counter changes.
//!
//! ## Why this is realtime-safe
//!
//! Swapping an `Arc` pointer is cheap, but *dropping* the last reference runs
//! the deallocator — never acceptable on the audio thread. Two mechanisms keep
//! the audio thread allocation-free:
//!
//! 1. **`try_lock` with fallback.** The shared `Arc` lives behind a `Mutex`,
//!    but the audio thread only ever `try_lock`s it, and only when the cheap
//!    generation counter says something changed. If the (briefly-held, control
//!    thread) lock is contended, the reader keeps its cached buffer for this
//!    sample and retries next sample. The audio thread never blocks.
//! 2. **Control-side retention.** When the control thread swaps in a new value
//!    it keeps the displaced `Arc`s alive in a small ring. By the time an old
//!    buffer is finally dropped (after several later swaps), every reader has
//!    long since refreshed past it, so the reader's own drop of its cached copy
//!    is never the last reference — the final drop happens on the control
//!    thread, inside `store`.
//!
//! `store` must therefore be called from a non-realtime thread.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

/// How many displaced payloads the control side keeps alive before dropping the
/// oldest. Each entry survives this many *subsequent* `store` calls; since the
/// audio thread refreshes within a sample or two of a swap, this is generous.
const RETIREMENT_DEPTH: usize = 8;

struct SlotInner<T> {
    /// Bumped on every `store`. Readers compare against their cached value to
    /// decide whether to refresh.
    generation: AtomicUsize,
    /// The live payload. `None` until the first `store`.
    current: Mutex<Option<Arc<T>>>,
    /// Displaced payloads kept alive so the audio thread never deallocates.
    /// Only ever touched by `store` (control thread).
    retired: Mutex<VecDeque<Arc<T>>>,
}

/// A cloneable handle to one realtime-swappable payload.
pub struct SampleSlot<T> {
    inner: Arc<SlotInner<T>>,
}

impl<T> Clone for SampleSlot<T> {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

impl<T> std::fmt::Debug for SampleSlot<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SampleSlot")
            .field("generation", &self.generation())
            .finish()
    }
}

impl<T> Default for SampleSlot<T> {
    fn default() -> Self {
        Self::empty()
    }
}

impl<T> SampleSlot<T> {
    /// An empty slot. Readers see `None` until the first [`store`](Self::store).
    pub fn empty() -> Self {
        Self {
            inner: Arc::new(SlotInner {
                generation: AtomicUsize::new(0),
                current: Mutex::new(None),
                retired: Mutex::new(VecDeque::new()),
            }),
        }
    }

    /// A slot pre-populated with `value`.
    pub fn new(value: Arc<T>) -> Self {
        let slot = Self::empty();
        slot.store(value);
        slot
    }

    /// Current generation counter. A reader that cached generation `g` should
    /// refresh when this differs from `g`. Cheap (a single atomic load).
    #[inline]
    pub fn generation(&self) -> usize {
        self.inner.generation.load(Ordering::Acquire)
    }

    /// Swap in a new payload. **Control thread only** — this takes a lock and
    /// may run destructors. The displaced payload is retained briefly so
    /// realtime readers never deallocate.
    pub fn store(&self, value: Arc<T>) {
        let displaced = {
            let mut guard = self.inner.current.lock().unwrap();
            guard.replace(value)
        };
        // Publish the change after the new value is in place.
        self.inner.generation.fetch_add(1, Ordering::Release);

        if let Some(old) = displaced {
            let mut retired = self.inner.retired.lock().unwrap();
            retired.push_back(old);
            while retired.len() > RETIREMENT_DEPTH {
                // Dropped here, on the control thread.
                retired.pop_front();
            }
        }
    }

    /// Clear the slot (readers see `None` after refreshing).
    pub fn clear(&self) {
        let displaced = {
            let mut guard = self.inner.current.lock().unwrap();
            guard.take()
        };
        self.inner.generation.fetch_add(1, Ordering::Release);
        if let Some(old) = displaced {
            self.inner.retired.lock().unwrap().push_back(old);
        }
    }

    /// Non-blocking realtime read. Returns:
    /// - `None` if the lock was momentarily contended (caller keeps its cache),
    /// - `Some(None)` / `Some(Some(arc))` if a fresh snapshot was obtained.
    ///
    /// Safe to call from the audio thread: it never blocks and never allocates
    /// (the `Arc` clone is a refcount bump).
    #[inline]
    pub fn try_load(&self) -> Option<Option<Arc<T>>> {
        match self.inner.current.try_lock() {
            Ok(guard) => Some(guard.clone()),
            Err(_) => None,
        }
    }

    /// Blocking load of the current payload. Convenient off the audio thread.
    pub fn load(&self) -> Option<Arc<T>> {
        self.inner.current.lock().unwrap().clone()
    }
}

/// A named registry of [`SampleSlot`]s — the "`buffer~`" identity layer.
///
/// Nodes reference a buffer by name (a string literal, which keeps them usable
/// inside the `graph!` macro), and the control thread swaps the data behind that
/// name with [`store`](Self::store). Every node holding the same name sees the
/// swap. Slot handles are created lazily, so a node can ask for `"kick"` before
/// any data has been loaded into it.
#[derive(Debug, Default)]
pub struct SampleBank<T = super::SampleBuffer> {
    slots: Mutex<std::collections::HashMap<String, SampleSlot<T>>>,
}

impl<T> SampleBank<T> {
    pub fn new() -> Self {
        Self {
            slots: Mutex::new(std::collections::HashMap::new()),
        }
    }

    /// Get the slot handle for `name`, creating an empty one if it doesn't
    /// exist yet. The returned handle is cheap to clone and share.
    pub fn slot(&self, name: &str) -> SampleSlot<T> {
        let mut slots = self.slots.lock().unwrap();
        slots
            .entry(name.to_owned())
            .or_insert_with(SampleSlot::empty)
            .clone()
    }

    /// Load (or swap) the payload behind `name`. Creates the slot if needed.
    /// **Control thread only.**
    pub fn store(&self, name: &str, value: Arc<T>) {
        self.slot(name).store(value);
    }

    /// Whether a slot has been created for `name` (it may still be empty).
    pub fn contains(&self, name: &str) -> bool {
        self.slots.lock().unwrap().contains_key(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_slot_reads_none() {
        let slot: SampleSlot<u32> = SampleSlot::empty();
        assert_eq!(slot.try_load(), Some(None));
        assert_eq!(slot.generation(), 0);
    }

    #[test]
    fn store_bumps_generation_and_publishes() {
        let slot = SampleSlot::empty();
        slot.store(Arc::new(42u32));
        assert_eq!(slot.generation(), 1);
        assert_eq!(*slot.load().unwrap(), 42);
    }

    #[test]
    fn swap_replaces_value() {
        let slot = SampleSlot::new(Arc::new(1u32));
        let g1 = slot.generation();
        slot.store(Arc::new(2u32));
        assert!(slot.generation() > g1);
        assert_eq!(*slot.load().unwrap(), 2);
    }

    #[test]
    fn retained_buffer_not_dropped_immediately() {
        // The displaced Arc must outlive a reader that still holds a clone.
        let slot = SampleSlot::new(Arc::new(1u32));
        let reader_copy = slot.load().unwrap(); // pretend the audio thread cached this
        assert_eq!(Arc::strong_count(&reader_copy), 2); // slot + reader

        slot.store(Arc::new(2u32)); // old value displaced but retained
        assert_eq!(Arc::strong_count(&reader_copy), 2); // retained, not dropped

        drop(reader_copy);
        // Now only the control-side retention holds the old value; dropping the
        // slot (control thread) releases it without ever touching a reader.
    }

    #[test]
    fn bank_shares_slot_by_name() {
        let bank: SampleBank<u32> = SampleBank::new();
        let a = bank.slot("kick");
        let b = bank.slot("kick");
        bank.store("kick", Arc::new(7u32));
        assert_eq!(*a.load().unwrap(), 7);
        assert_eq!(*b.load().unwrap(), 7); // same underlying slot
    }
}
