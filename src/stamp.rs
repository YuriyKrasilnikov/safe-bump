use std::cell::RefCell;
use std::num::NonZeroU64;
use std::sync::atomic::{AtomicU64, Ordering};

#[cfg(not(target_has_atomic = "64"))]
compile_error!("safe-bump currently requires native 64-bit atomics for allocation stamps");

/// Process-unique, non-zero identity for an arena or allocation block.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Stamp(NonZeroU64);

static NEXT_STAMP: AtomicU64 = AtomicU64::new(1);

// Amortize the global uniqueness transition across every arena used by one
// thread. Reserved-but-unused values are never returned: capabilities may
// outlive an arena or move between threads, so recycling would reintroduce ABA.
const LOCAL_RESERVATION_LEN: u64 = 256;

thread_local! {
    static LOCAL_STAMPS: RefCell<StampPool> = const { RefCell::new(StampPool::new()) };
}

impl Stamp {
    /// Returns a stamp that has never previously been returned in this process.
    pub(crate) fn fresh() -> Self {
        LOCAL_STAMPS.with(|pool| {
            let mut state = pool
                .try_borrow_mut()
                .expect("stamp generation must not be re-entered");
            state.fresh()
        })
    }

    pub(crate) const fn get(self) -> u64 {
        self.0.get()
    }

    const fn from_raw(raw: u64) -> Self {
        Self(NonZeroU64::new(raw).expect("the stamp sequence starts at one"))
    }
}

struct StampPool {
    next: u64,
    end: u64,
}

impl StampPool {
    const fn new() -> Self {
        Self { next: 0, end: 0 }
    }

    fn fresh(&mut self) -> Stamp {
        if self.next == self.end {
            (self.next, self.end) = reserve_raw(LOCAL_RESERVATION_LEN);
        }
        let raw = self.next;
        self.next = self
            .next
            .checked_add(1)
            .expect("a reserved stamp range ends before u64 overflow");
        Stamp::from_raw(raw)
    }
}

fn reserve_raw(len: u64) -> (u64, u64) {
    assert!(len > 0, "a stamp reservation must be non-empty");
    let start = NEXT_STAMP
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
            current.checked_add(len)
        })
        .unwrap_or_else(|_| panic!("safe-bump allocation stamp space exhausted"));
    let end = start
        .checked_add(len)
        .expect("the successful atomic reservation has a representable end");
    (start, end)
}

#[cfg(test)]
mod tests {
    use std::panic::{AssertUnwindSafe, catch_unwind};

    use super::{LOCAL_RESERVATION_LEN, LOCAL_STAMPS, Stamp};

    #[test]
    fn reentrant_generation_fails_loud_without_corrupting_state() {
        LOCAL_STAMPS.with(|pool| {
            let held_state = pool.borrow_mut();
            let result = catch_unwind(AssertUnwindSafe(Stamp::fresh));
            assert!(result.is_err(), "reentrant generation must be rejected");
            drop(held_state);
        });

        let _fresh_after_rejection = Stamp::fresh();
    }

    #[test]
    fn thread_local_ranges_are_process_unique() {
        const THREADS: usize = 4;
        let per_thread = usize::try_from(LOCAL_RESERVATION_LEN + 1)
            .expect("the reservation test count fits usize");
        let mut stamps: Vec<_> = (0..per_thread).map(|_| Stamp::fresh().get()).collect();
        std::thread::scope(|scope| {
            let handles: Vec<_> = (0..THREADS)
                .map(|_| {
                    scope.spawn(|| {
                        (0..per_thread)
                            .map(|_| Stamp::fresh().get())
                            .collect::<Vec<_>>()
                    })
                })
                .collect();
            for handle in handles {
                stamps.extend(handle.join().expect("stamp worker does not panic"));
            }
        });
        let expected_len = per_thread * (THREADS + 1);
        assert_eq!(stamps.len(), expected_len);
        stamps.sort_unstable();
        stamps.dedup();
        assert_eq!(stamps.len(), expected_len);
    }

    #[test]
    fn unused_thread_local_tail_is_never_recycled() {
        let first = std::thread::spawn(|| Stamp::fresh().get())
            .join()
            .expect("stamp worker does not panic");
        let second = std::thread::spawn(|| Stamp::fresh().get())
            .join()
            .expect("stamp worker does not panic");
        assert_ne!(first, second);
        assert!(
            first.abs_diff(second) >= LOCAL_RESERVATION_LEN,
            "a new thread receives a disjoint reservation"
        );
    }
}
