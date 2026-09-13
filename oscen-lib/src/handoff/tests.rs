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

#[test]
fn idle_take_after_take_is_none_and_publish_rearms() {
    let drops = Arc::new(AtomicUsize::new(0));
    let (mut pubr, mut cons) = pair::<Tracked>();

    assert!(cons.take().is_none(), "nothing published yet");
    pubr.publish(Tracked {
        id: 1,
        drops: drops.clone(),
    });
    assert_eq!(cons.take().map(|t| t.id), Some(1));
    assert!(cons.take().is_none());
    assert!(cons.take().is_none());

    // Two publishes back to back: newest wins, exactly one take succeeds.
    pubr.publish(Tracked {
        id: 2,
        drops: drops.clone(),
    });
    pubr.publish(Tracked {
        id: 3,
        drops: drops.clone(),
    });
    assert_eq!(cons.take().map(|t| t.id), Some(3));
    assert!(cons.take().is_none());
}

#[test]
fn concurrent_publish_and_take_never_duplicates_or_loses_the_last_value() {
    use std::sync::atomic::AtomicBool;
    use std::thread;

    const N: u32 = 2_000;
    let (mut pubr, mut cons) = pair::<u32>();
    let done = Arc::new(AtomicBool::new(false));

    let producer = {
        let done = done.clone();
        thread::spawn(move || {
            for i in 1..=N {
                pubr.publish(i);
                if i % 7 == 0 {
                    thread::yield_now();
                }
            }
            done.store(true, Ordering::SeqCst);
            pubr
        })
    };

    // Poll like an audio thread would, until the producer is done and the
    // final value has been observed.
    let mut seen = Vec::new();
    let mut last = 0u32;
    loop {
        if let Some(v) = cons.take() {
            assert!(*v > last, "values must arrive in publish order");
            last = *v;
            seen.push(*v);
            cons.retire(v);
        }
        if done.load(Ordering::SeqCst) {
            // One more take: the final publish must be observable now.
            if let Some(v) = cons.take() {
                assert!(*v > last);
                last = *v;
                seen.push(*v);
                cons.retire(v);
            }
            break;
        }
        thread::yield_now();
    }
    let _pubr = producer.join().unwrap();
    assert_eq!(last, N, "the newest published value must always arrive");
    assert!(seen.len() as u32 <= N, "never more takes than publishes");
    assert!(cons.take().is_none());
}
