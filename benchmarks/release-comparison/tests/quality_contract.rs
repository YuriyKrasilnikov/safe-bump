use std::cell::Cell;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;

use safe_bump_current::{Arena as CurrentArena, CheckpointError, SharedArena as CurrentShared};
use safe_bump_v2::{Arena as PreviousArena, SharedArena as PreviousShared};

#[test]
fn v03_rejects_foreign_indices_that_v02_cannot_distinguish() {
    let mut previous_left = PreviousArena::new();
    let previous_foreign = previous_left.alloc(10_u64);
    let mut previous_right = PreviousArena::new();
    previous_right.alloc(20_u64);
    assert_eq!(previous_right.try_get(previous_foreign), Some(&20));

    let mut current_left = CurrentArena::new();
    let current_foreign = current_left.alloc(10_u64);
    let mut current_right = CurrentArena::new();
    current_right.alloc(20_u64);
    assert_eq!(current_right.try_get(current_foreign), None);
}

#[test]
fn v03_rejects_stale_aba_indices_that_v02_retargets() {
    let mut previous = PreviousArena::new();
    previous.alloc(1_u64);
    let previous_checkpoint = previous.checkpoint();
    let previous_stale = previous.alloc(2);
    previous.rollback(previous_checkpoint);
    previous.alloc(3);
    assert_eq!(previous.try_get(previous_stale), Some(&3));

    let mut current = CurrentArena::new();
    current.alloc(1_u64);
    let current_checkpoint = current.checkpoint();
    let current_stale = current.alloc(2);
    current.rollback(current_checkpoint);
    let current_replacement = current.alloc(3);
    assert_eq!(current.try_get(current_stale), None);
    assert_eq!(current.try_get(current_replacement), Some(&3));
}

#[test]
fn v03_rejects_foreign_checkpoints_that_v02_accepts_by_length() {
    let previous_left = PreviousArena::<u64>::new();
    let previous_foreign = previous_left.checkpoint();
    let mut previous_right = PreviousArena::new();
    previous_right.alloc(2_u64);
    previous_right.rollback(previous_foreign);
    assert!(previous_right.is_empty());

    let current_left = CurrentArena::<u64>::new();
    let current_foreign = current_left.checkpoint();
    let mut current_right = CurrentArena::new();
    current_right.alloc(2_u64);
    assert_eq!(
        current_right.try_rollback(current_foreign),
        Err(CheckpointError::ForeignArena)
    );
    assert_eq!(current_right.len(), 1);
}

// A v0.2.1 `SharedArena::rollback` writes each dropped slot's value out (running its
// destructor) before updating the `published`/`reserved` counters for that slot. A
// destructor panic therefore leaves the counters exactly where they were before the
// rollback started, over storage that already lost the slot's value: the stale index
// still reports valid, but reading through it panics. v0.3.0 updates a slot's counters
// before running that slot's destructor, so the same destructor panic leaves the arena
// in a state whose counters and storage agree.
#[test]
fn v03_shared_rollback_stays_consistent_after_a_panicking_drop_that_v02_corrupts_publication() {
    struct Tracked {
        drops: Rc<Cell<u32>>,
        panic_on_drop: bool,
    }

    impl Drop for Tracked {
        fn drop(&mut self) {
            self.drops.set(self.drops.get() + 1);
            if self.panic_on_drop {
                panic!("intentional panic in Drop");
            }
        }
    }

    fn tracked(drops: &Rc<Cell<u32>>, panic_on_drop: bool) -> Tracked {
        Tracked {
            drops: Rc::clone(drops),
            panic_on_drop,
        }
    }

    let previous_drops = Rc::new(Cell::new(0));
    let mut previous = PreviousShared::new();
    previous.alloc(tracked(&previous_drops, false));
    let previous_checkpoint = previous.checkpoint();
    let previous_dropped = previous.alloc(tracked(&previous_drops, true));

    let previous_result = catch_unwind(AssertUnwindSafe(|| previous.rollback(previous_checkpoint)));
    assert!(previous_result.is_err());
    assert_eq!(
        previous.len(),
        2,
        "v0.2.1: published still reports the pre-rollback length after the failed rollback"
    );
    assert!(
        previous.is_valid(previous_dropped),
        "v0.2.1: the dropped value's index is still reported valid"
    );
    let previous_get = catch_unwind(AssertUnwindSafe(|| {
        previous.try_get(previous_dropped).is_some()
    }));
    assert!(
        previous_get.is_err(),
        "v0.2.1: try_get on the 'valid' index panics because the slot is empty"
    );
    let previous_iter = catch_unwind(AssertUnwindSafe(|| previous.iter().count()));
    assert!(
        previous_iter.is_err(),
        "v0.2.1: iter panics while reading the empty published slot"
    );

    let current_drops = Rc::new(Cell::new(0));
    let mut current = CurrentShared::new();
    let current_kept = current.alloc(tracked(&current_drops, false));
    let current_checkpoint = current.checkpoint();
    let current_dropped = current.alloc(tracked(&current_drops, true));

    let current_result = catch_unwind(AssertUnwindSafe(|| current.rollback(current_checkpoint)));
    assert!(current_result.is_err());
    assert_eq!(current.len(), 1);
    assert!(current.is_valid(current_kept));
    assert!(!current.is_valid(current_dropped));
    assert!(current.try_get(current_dropped).is_none());
    assert_eq!(current.iter().count(), 1);
}

// A v0.2.1 `SharedArena::alloc_extend` reserves and publishes one slot per yielded item
// through repeated `&self` calls to `alloc`, so a reentrant (or, in general, concurrent)
// allocation that happens between two yields lands inside the range the caller believes
// is its own contiguous batch. v0.3.0's `SharedArena::alloc_block` collects the input
// first and reserves the whole range in a single atomic step, so the returned `Block`
// cannot be interleaved by another allocation.
#[test]
fn v03_shared_alloc_block_reserves_one_contiguous_range_that_v02_alloc_extend_lets_interleave() {
    struct Reentrant<'a, A, F: Fn(&A)> {
        arena: &'a A,
        hook: F,
        yielded: usize,
    }

    impl<A, F: Fn(&A)> Iterator for Reentrant<'_, A, F> {
        type Item = u64;

        fn next(&mut self) -> Option<u64> {
            self.yielded += 1;
            match self.yielded {
                1 => Some(10),
                2 => {
                    (self.hook)(self.arena);
                    Some(20)
                }
                _ => None,
            }
        }
    }

    let previous = PreviousShared::<u64>::new();
    let previous_first = previous
        .alloc_extend(Reentrant {
            arena: &previous,
            hook: |arena: &PreviousShared<u64>| {
                arena.alloc(999);
            },
            yielded: 0,
        })
        .expect("alloc_extend yields at least one item");
    let previous_second = safe_bump_v2::Idx::<u64>::from_raw(previous_first.into_raw() + 1);
    assert_eq!(*previous.get(previous_first), 10);
    assert_eq!(
        *previous.get(previous_second),
        999,
        "v0.2.1: the slot right after the batch's first item is the interleaved \
         allocation, not the batch's second item"
    );
    assert_eq!(previous.len(), 3);
    let previous_batch_second = safe_bump_v2::Idx::<u64>::from_raw(previous_first.into_raw() + 2);
    assert_eq!(
        *previous.get(previous_batch_second),
        20,
        "v0.2.1: the batch's own second item lands at first + 2, one slot beyond \
         where the caller would expect a contiguous two-item batch to end"
    );

    let current = CurrentShared::<u64>::new();
    let current_block = current.alloc_block(Reentrant {
        arena: &current,
        hook: |arena: &CurrentShared<u64>| {
            arena.alloc(999);
        },
        yielded: 0,
    });
    assert_eq!(current_block.len(), 2);
    assert_eq!(*current.get(current_block.get(0).unwrap()), 10);
    assert_eq!(
        *current.get(current_block.get(1).unwrap()),
        20,
        "v0.3.0: the block's slots are contiguous and unaffected by the reentrant allocation"
    );
    assert_eq!(current.len(), 3);
}
