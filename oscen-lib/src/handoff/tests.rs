//! Unit tests for the RT handoff primitive. Single-threaded and deterministic:
//! the SPSC logic is identical run on one thread, and determinism lets the tests
//! assert *where* destruction happens via a drop counter.

use super::pair;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

/// Drop-counting payload so tests can assert which side runs the destructor.
struct Tracked {
    id: u32,
    drops: Arc<AtomicUsize>,
}

impl Drop for Tracked {
    fn drop(&mut self) {
        self.drops.fetch_add(1, Ordering::SeqCst);
    }
}

#[test]
fn handoff_take_returns_each_publish_once() {
    let drops = Arc::new(AtomicUsize::new(0));
    let (mut pubr, mut cons) = pair::<Tracked>();

    pubr.publish(Tracked {
        id: 1,
        drops: drops.clone(),
    });

    let first = cons.take();
    assert!(first.is_some());
    assert_eq!(first.unwrap().id, 1);

    // No intervening publish: the slot is empty again.
    assert!(cons.take().is_none());

    pubr.publish(Tracked {
        id: 2,
        drops: drops.clone(),
    });
    let second = cons.take();
    assert!(second.is_some());
    assert_eq!(second.unwrap().id, 2);
}

#[test]
fn handoff_newest_publish_wins_and_drops_stale() {
    let drops = Arc::new(AtomicUsize::new(0));
    let (mut pubr, mut cons) = pair::<Tracked>();

    pubr.publish(Tracked {
        id: 1,
        drops: drops.clone(),
    });
    // Second publish without an intervening take: value 1 is displaced and
    // dropped on the producer side.
    pubr.publish(Tracked {
        id: 2,
        drops: drops.clone(),
    });

    assert_eq!(drops.load(Ordering::SeqCst), 1);

    let taken = cons.take();
    assert!(taken.is_some());
    assert_eq!(taken.unwrap().id, 2);
}

#[test]
fn handoff_retired_value_dropped_on_producer_side() {
    let drops = Arc::new(AtomicUsize::new(0));
    let (mut pubr, mut cons) = pair::<Tracked>();

    pubr.publish(Tracked {
        id: 1,
        drops: drops.clone(),
    });
    let arc = cons.take().expect("value published");

    // Hand the retired value back; it sits in the return ring, not dropped yet.
    cons.retire(arc);
    assert_eq!(drops.load(Ordering::SeqCst), 0);

    // The next publish drains the return ring and drops the retired value
    // off the audio thread.
    pubr.publish(Tracked {
        id: 2,
        drops: drops.clone(),
    });
    assert_eq!(drops.load(Ordering::SeqCst), 1);
}

#[test]
fn handoff_take_is_none_before_first_publish() {
    let (_pubr, mut cons) = pair::<Tracked>();
    assert!(cons.take().is_none());
}
