use std::cell::Cell;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Barrier, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use super::*;

#[test]
fn is_send_and_sync_when_values_are() {
    fn assert_send<T: Send>() {}
    fn assert_sync<T: Sync>() {}

    assert_send::<SharedArena<String>>();
    assert_sync::<SharedArena<String>>();
}

#[test]
fn alloc_uses_a_shared_reference_and_get_returns_a_plain_reference() {
    let arena = SharedArena::new();
    let first = arena.alloc(10);
    let second = arena.alloc(20);
    let value: &i32 = arena.get(second);

    assert_eq!(*arena.get(first), 10);
    assert_eq!(*value, 20);
    assert_eq!(arena.len(), 2);
}

#[test]
fn foreign_equal_slot_is_rejected() {
    let left = SharedArena::new();
    let right = SharedArena::new();
    let left_idx = left.alloc(10);
    let right_idx = right.alloc(20);

    assert_eq!(left_idx.slot(), right_idx.slot());
    assert_ne!(left_idx, right_idx);
    assert_eq!(left.try_get(right_idx), None);
    assert_eq!(right.try_get(left_idx), None);
}

#[test]
fn reset_reuse_does_not_resurrect_an_index() {
    let mut arena = SharedArena::new();
    let stale = arena.alloc(String::from("old"));
    arena.reset();
    let current = arena.alloc(String::from("new"));

    assert_eq!(stale.slot(), current.slot());
    assert_eq!(arena.try_get(stale), None);
    assert_eq!(arena.try_get(current).map(String::as_str), Some("new"));
}

#[test]
#[should_panic(expected = "foreign, stale, or unpublished")]
fn indexing_with_a_stale_capability_panics() {
    let mut arena = SharedArena::new();
    let stale = arena.alloc(1);
    arena.reset();
    let _ = arena[stale];
}

#[test]
fn block_is_contiguous_and_bounds_checked() {
    let arena = SharedArena::new();
    let _prefix = arena.alloc(0);
    let block = arena.alloc_block([10, 20, 30]);

    assert_eq!(block.len(), 3);
    assert_eq!(block.first().map(Idx::slot), Some(1));
    assert_eq!(block.last().map(Idx::slot), Some(3));
    assert_eq!(block.get(3), None);
    let values: Vec<_> = block.indices().map(|idx| *arena.get(idx)).collect();
    assert_eq!(values, vec![10, 20, 30]);
    assert!(block.indices().all(|idx| block.contains(idx)));
}

#[test]
fn empty_block_does_not_change_publication() {
    let arena = SharedArena::<i32>::new();
    let block = arena.alloc_block(std::iter::empty());
    assert!(block.is_empty());
    assert!(arena.is_empty());
}

// Characterization test for why `alloc_block`'s panic guarantee is scoped to
// its own reservation/publication rather than the whole arena: since
// `SharedArena` takes `&self`, a caller-supplied iterator can reenter the
// same arena and allocate before it panics, and that reentrant allocation
// publishes immediately and is not undone by the later panic.
#[test]
fn alloc_block_reentrant_iterator_allocation_survives_the_input_panic() {
    struct ReentrantThenPanic<'a> {
        arena: &'a SharedArena<i32>,
        recorded: Rc<Cell<Option<Idx<i32>>>>,
        yielded: usize,
    }

    impl Iterator for ReentrantThenPanic<'_> {
        type Item = i32;

        fn next(&mut self) -> Option<i32> {
            if self.yielded == 2 {
                let reentrant = self.arena.alloc(999);
                self.recorded.set(Some(reentrant));
                panic!("input iterator panics after a reentrant allocation");
            }
            self.yielded += 1;
            Some(10)
        }
    }

    let arena = SharedArena::new();
    let baseline = arena.alloc(0);
    let recorded: Rc<Cell<Option<Idx<i32>>>> = Rc::new(Cell::new(None));
    let source = ReentrantThenPanic {
        arena: &arena,
        recorded: Rc::clone(&recorded),
        yielded: 0,
    };

    let result =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| arena.alloc_block(source)));

    assert!(result.is_err(), "the input iterator was expected to panic");
    let reentrant_idx = recorded
        .get()
        .expect("the iterator records its reentrant allocation before panicking");

    // Only the baseline allocation and the iterator's own reentrant
    // allocation are published; `alloc_block`'s own reservation for the
    // block being collected never happened.
    assert_eq!(arena.len(), 2);
    assert!(arena.is_valid(baseline));
    assert!(arena.is_valid(reentrant_idx));
    assert_eq!(*arena.get(reentrant_idx), 999);

    // The interrupted `alloc_block` panicked while still collecting its
    // input into a `Vec`, before calling `reserve_range` at all, so it did
    // not leak a reservation that a later allocation would have to wait
    // behind or skip over. Confirm a follow-up `alloc` lands right after the
    // reentrant allocation and is fully usable.
    let after = arena.alloc(7);
    assert_eq!(arena.len(), 3);
    assert!(arena.is_valid(after));
    assert_eq!(*arena.get(after), 7);
}

#[test]
fn checkpoint_rollback_preserves_the_prefix_and_drops_the_suffix() {
    let drops = Rc::new(Cell::new(0));
    let mut arena = SharedArena::new();
    let kept = arena.alloc(Tracked(Rc::clone(&drops)));
    let checkpoint = arena.checkpoint();
    let stale = arena.alloc(Tracked(Rc::clone(&drops)));

    arena.rollback(checkpoint);
    assert_eq!(arena.len(), 1);
    assert!(arena.is_valid(kept));
    assert!(!arena.is_valid(stale));
    assert_eq!(drops.get(), 1);
}

#[test]
fn foreign_and_diverged_checkpoints_are_rejected() {
    let foreign_arena: SharedArena<i32> = SharedArena::new();
    let foreign = foreign_arena.checkpoint();
    let mut arena = SharedArena::new();
    let _root = arena.alloc(0);
    assert_eq!(
        arena.try_rollback(foreign),
        Err(CheckpointError::ForeignArena)
    );

    let branch = arena.checkpoint();
    let stale = arena.alloc(1);
    let stale_prefix = arena.checkpoint();
    arena.rollback(branch);
    let current = arena.alloc(2);

    assert_eq!(stale.slot(), current.slot());
    assert_eq!(
        arena.try_rollback(stale_prefix),
        Err(CheckpointError::DivergedPrefix { checkpoint_len: 2 })
    );
    assert_eq!(*arena.get(current), 2);
}

#[test]
fn validate_checkpoint_accepts_a_valid_checkpoint_without_mutating_the_arena() {
    let arena = SharedArena::new();
    arena.alloc(1);
    arena.alloc(2);
    let checkpoint = arena.checkpoint();
    arena.alloc(3);

    assert_eq!(arena.validate_checkpoint(checkpoint), Ok(()));
    assert_eq!(arena.len(), 3, "validation must not mutate the arena");
}

#[test]
fn validate_checkpoint_rejects_a_foreign_checkpoint() {
    let foreign_arena: SharedArena<i32> = SharedArena::new();
    let foreign = foreign_arena.checkpoint();
    let arena: SharedArena<i32> = SharedArena::new();

    assert_eq!(
        arena.validate_checkpoint(foreign),
        Err(CheckpointError::ForeignArena)
    );
}

#[test]
fn validate_checkpoint_rejects_a_checkpoint_beyond_current_length() {
    let mut arena = SharedArena::new();
    arena.alloc(1);
    arena.alloc(2);
    let cp_early = arena.checkpoint(); // saves len=2
    arena.alloc(3);
    arena.alloc(4);
    arena.alloc(5);
    let cp_late = arena.checkpoint(); // saves len=5
    arena.rollback(cp_early); // back to len=2

    assert_eq!(
        arena.validate_checkpoint(cp_late),
        Err(CheckpointError::BeyondCurrent {
            checkpoint_len: 5,
            current_len: 2,
        })
    );
}

#[test]
fn validate_checkpoint_rejects_a_diverged_prefix() {
    let mut arena = SharedArena::new();
    let _root = arena.alloc(0);
    let branch = arena.checkpoint();
    let _stale = arena.alloc(1);
    let stale_prefix = arena.checkpoint();
    arena.rollback(branch);
    let _current = arena.alloc(2);

    assert_eq!(
        arena.validate_checkpoint(stale_prefix),
        Err(CheckpointError::DivergedPrefix { checkpoint_len: 2 })
    );
}

#[test]
fn validate_checkpoint_accepts_an_empty_arena_checkpoint_after_reset() {
    let mut arena = SharedArena::new();
    arena.alloc(1);
    arena.alloc(2);
    arena.reset();
    let empty_checkpoint = arena.checkpoint();

    assert_eq!(empty_checkpoint.len(), 0);
    assert!(empty_checkpoint.tail().is_none());
    assert_eq!(arena.validate_checkpoint(empty_checkpoint), Ok(()));
}

/// `SharedArena` counterpart of
/// `tests::arena::is_valid_agrees_with_a_brute_force_oracle_across_many_increasing_rollbacks`:
/// builds several archived segments in `SharedIdentity` by repeatedly
/// rolling back to a fresh checkpoint whose length exceeds the start of the
/// segment just discarded — the O(1) "growing" branch of
/// `crate::segments::SharedIdentity::commit`'s `prefix_tail` maintenance —
/// then cross-checks `is_valid`, through the public surface only, against a
/// brute-force oracle tracked independently of the arena's own bookkeeping.
#[test]
fn is_valid_agrees_with_a_brute_force_oracle_across_many_increasing_rollbacks() {
    fn alloc_at(
        arena: &SharedArena<usize>,
        slot_epoch: &mut Vec<u64>,
        handles: &mut Vec<(Idx<usize>, usize, u64)>,
        epoch: u64,
    ) {
        let slot = arena.len();
        let idx = arena.alloc(slot);
        if slot < slot_epoch.len() {
            slot_epoch[slot] = epoch;
        } else {
            slot_epoch.push(epoch);
        }
        handles.push((idx, slot, epoch));
    }

    let mut arena: SharedArena<usize> = SharedArena::new();
    let mut slot_epoch: Vec<u64> = Vec::new();
    let mut handles: Vec<(Idx<usize>, usize, u64)> = Vec::new();
    let mut epoch: u64 = 0;

    for _ in 0..6 {
        alloc_at(&arena, &mut slot_epoch, &mut handles, epoch);
    }
    let cp_at_6 = arena.checkpoint();
    for _ in 0..9 {
        alloc_at(&arena, &mut slot_epoch, &mut handles, epoch);
    }
    assert_eq!(arena.len(), 15);

    // First rollback: the very first commit this identity ever sees, so it
    // archives nothing (the `SharedIdentity::commit` branch that leaves
    // `prefix_tail` at `birth`).
    arena.rollback(cp_at_6);
    epoch += 1;
    for _ in 0..3 {
        alloc_at(&arena, &mut slot_epoch, &mut handles, epoch);
    }
    let cp_at_9 = arena.checkpoint();
    for _ in 0..3 {
        alloc_at(&arena, &mut slot_epoch, &mut handles, epoch);
    }
    assert_eq!(arena.len(), 12);

    // Target (9) exceeds the segment just discarded's start (6): the O(1)
    // growing branch, appending the table's first real entry.
    arena.rollback(cp_at_9);
    epoch += 1;
    for _ in 0..3 {
        alloc_at(&arena, &mut slot_epoch, &mut handles, epoch);
    }
    let cp_at_12 = arena.checkpoint();
    for _ in 0..3 {
        alloc_at(&arena, &mut slot_epoch, &mut handles, epoch);
    }
    assert_eq!(arena.len(), 15);

    // Target (12) exceeds the previous start (9): a second table entry.
    arena.rollback(cp_at_12);
    epoch += 1;
    for _ in 0..3 {
        alloc_at(&arena, &mut slot_epoch, &mut handles, epoch);
    }
    let cp_at_15 = arena.checkpoint();
    for _ in 0..3 {
        alloc_at(&arena, &mut slot_epoch, &mut handles, epoch);
    }
    assert_eq!(arena.len(), 18);

    // Target (15) exceeds the previous start (12): a third table entry.
    arena.rollback(cp_at_15);
    epoch += 1;
    for _ in 0..2 {
        alloc_at(&arena, &mut slot_epoch, &mut handles, epoch);
    }
    assert_eq!(arena.len(), 17);

    for (idx, slot, alloc_epoch) in &handles {
        let current_owner = slot_epoch.get(*slot).copied();
        let expected = *slot < arena.len() && current_owner == Some(*alloc_epoch);
        assert_eq!(
            arena.is_valid(*idx),
            expected,
            "slot {slot} allocated at epoch {alloc_epoch}, now owned by {current_owner:?}, \
             current len {}",
            arena.len()
        );
    }
}

/// `SharedArena` counterpart of
/// `tests::arena::checkpoint_right_after_rollback_round_trips_then_detects_a_rebuilt_prefix`.
/// A checkpoint captured immediately after a rollback lands exactly on the
/// one slot `SharedIdentity::commit`'s `prefix_tail` cache exists for: it
/// must still round-trip through `try_rollback`/`validate_checkpoint`, and
/// once the prefix it names is later discarded and rebuilt under a fresh
/// generation, the same checkpoint must be rejected as `DivergedPrefix`.
#[test]
fn checkpoint_right_after_rollback_round_trips_then_detects_a_rebuilt_prefix() {
    let mut arena = SharedArena::new();
    let a = arena.alloc(1);
    let b = arena.alloc(2);
    let mid = arena.checkpoint(); // len = 2
    arena.alloc(3);
    arena.alloc(4);

    arena.rollback(mid); // -> len 2; current_start becomes 2

    // Captured immediately after the rollback: `checkpoint()` asks for slot
    // 1, which is `current_start - 1` — the `prefix_tail` fast path.
    let just_after = arena.checkpoint();
    assert_eq!(just_after.len(), 2);

    assert_eq!(arena.validate_checkpoint(just_after), Ok(()));
    arena.rollback(just_after); // no values to drop, but still commits a
    // fresh generation for slot 1 (the non-growing, cold `prefix_tail`
    // branch, since the target length does not exceed the prior start).
    assert_eq!(arena.len(), 2);
    assert_eq!(arena[a], 1);
    assert_eq!(arena[b], 2);

    // Discard and rebuild the prefix itself under a new generation.
    arena.reset();
    arena.alloc(10);
    arena.alloc(20);
    assert_eq!(arena.len(), 2);

    assert_eq!(
        arena.validate_checkpoint(just_after),
        Err(CheckpointError::DivergedPrefix { checkpoint_len: 2 })
    );
    assert_eq!(
        arena.try_rollback(just_after),
        Err(CheckpointError::DivergedPrefix { checkpoint_len: 2 })
    );
}

/// `SharedArena` counterpart of
/// `tests::arena::checkpoint_after_reset_and_after_drain_are_both_empty_and_valid`.
/// `commit(0)` (via `reset` and via `drain`) followed immediately by a
/// `checkpoint`: `checkpoint()` computes `len.checked_sub(1)`, which is
/// `None` at `len == 0`, so it never reads `prefix_tail` at all.
#[test]
fn checkpoint_after_reset_and_after_drain_are_both_empty_and_valid() {
    let mut arena = SharedArena::new();
    arena.alloc(1);
    arena.alloc(2);
    arena.reset();
    let after_reset = arena.checkpoint();
    assert_eq!(after_reset.len(), 0);
    assert!(after_reset.tail().is_none());
    assert_eq!(arena.validate_checkpoint(after_reset), Ok(()));
    arena.rollback(after_reset);
    assert_eq!(arena.len(), 0);

    arena.alloc(10);
    arena.alloc(20);
    arena.alloc(30);
    let drained: Vec<_> = arena.drain().collect();
    assert_eq!(drained, vec![10, 20, 30]);
    let after_drain = arena.checkpoint();
    assert_eq!(after_drain.len(), 0);
    assert!(after_drain.tail().is_none());
    assert_eq!(arena.validate_checkpoint(after_drain), Ok(()));
    arena.rollback(after_drain);
    assert_eq!(arena.len(), 0);
}

#[test]
fn reset_preserves_drop_exactly_once() {
    let drops = Rc::new(Cell::new(0));
    let mut arena = SharedArena::new();
    arena.alloc(Tracked(Rc::clone(&drops)));
    arena.alloc(Tracked(Rc::clone(&drops)));
    arena.reset();

    assert_eq!(drops.get(), 2);
    assert!(arena.is_empty());
    drop(arena);
    assert_eq!(drops.get(), 2);
}

#[test]
fn iterators_and_drain_preserve_allocation_order() {
    let mut arena = SharedArena::new();
    let block = arena.alloc_block([10, 20, 30]);

    assert_eq!(arena.iter().copied().collect::<Vec<_>>(), vec![10, 20, 30]);
    assert_eq!(
        arena
            .iter_indexed()
            .map(|(idx, value)| (idx, *value))
            .collect::<Vec<_>>(),
        block.indices().zip([10, 20, 30]).collect::<Vec<_>>()
    );
    assert_eq!(arena.drain().collect::<Vec<_>>(), vec![10, 20, 30]);
    assert!(arena.is_empty());
}

#[test]
fn drain_empties_the_arena_and_is_reusable_afterward() {
    let mut arena = SharedArena::new();
    arena.alloc(1);
    arena.alloc(2);
    arena.alloc(3);

    assert_eq!(arena.drain().collect::<Vec<_>>(), vec![1, 2, 3]);
    assert_eq!(arena.len(), 0);
    assert_eq!(arena.drain().collect::<Vec<_>>(), Vec::<i32>::new());

    let after = arena.alloc(4);
    assert_eq!(arena.len(), 1);
    assert!(arena.is_valid(after));
    assert_eq!(*arena.get(after), 4);
}

/// `drain` commits a fresh segment stamp before it takes any slot (see
/// `Self::drain`'s doc comment), so slots regrown to the exact pre-drain
/// length belong to a new generation, not the old one: every pre-drain
/// `Idx` is rejected, a pre-drain `Checkpoint` whose length no longer
/// exceeds the regrown length is rejected as `DivergedPrefix` rather than
/// wrongly validating, and only the freshly minted indices are valid.
#[test]
fn drain_then_regrowth_to_the_same_length_rejects_every_pre_drain_capability() {
    let mut arena = SharedArena::new();
    let old = [arena.alloc(1), arena.alloc(2), arena.alloc(3)];
    let old_checkpoint = arena.checkpoint();

    let drained: Vec<_> = arena.drain().collect();
    assert_eq!(drained, vec![1, 2, 3]);

    let new = [arena.alloc(10), arena.alloc(20), arena.alloc(30)];

    assert_eq!(arena.len(), 3);
    for idx in old {
        assert!(!arena.is_valid(idx));
        assert_eq!(arena.try_get(idx), None);
    }
    assert_eq!(
        arena.validate_checkpoint(old_checkpoint),
        Err(CheckpointError::DivergedPrefix { checkpoint_len: 3 })
    );
    for (idx, expected) in new.into_iter().zip([10, 20, 30]) {
        assert!(arena.is_valid(idx));
        assert_eq!(arena.try_get(idx), Some(&expected));
    }
}

/// The same as
/// `drain_then_regrowth_to_the_same_length_rejects_every_pre_drain_capability`,
/// but the iterator `drain` returns is dropped without being consumed.
/// Every slot is already taken (and the fresh generation already committed)
/// by the time `drain` returns the iterator, regardless of whether the
/// caller ever reads it — unlike `Arena::drain`, whose `vec::Drain` only
/// removes values lazily as the caller consumes or drops it, `SharedArena`'s
/// `drain` unpublishes and takes every slot eagerly inside the call itself.
#[test]
fn drain_dropped_without_consuming_still_regrows_under_a_new_generation() {
    let mut arena = SharedArena::new();
    let old = [arena.alloc(1), arena.alloc(2), arena.alloc(3)];
    let old_checkpoint = arena.checkpoint();

    drop(arena.drain());
    assert_eq!(arena.len(), 0);

    let new = [arena.alloc(10), arena.alloc(20), arena.alloc(30)];

    assert_eq!(arena.len(), 3);
    for idx in old {
        assert!(!arena.is_valid(idx));
        assert_eq!(arena.try_get(idx), None);
    }
    assert_eq!(
        arena.validate_checkpoint(old_checkpoint),
        Err(CheckpointError::DivergedPrefix { checkpoint_len: 3 })
    );
    for (idx, expected) in new.into_iter().zip([10, 20, 30]) {
        assert!(arena.is_valid(idx));
        assert_eq!(arena.try_get(idx), Some(&expected));
    }
}

#[test]
fn extend_from_iter_and_default_share_the_block_semantics() {
    let mut extended = SharedArena::default();
    extended.extend([1, 2, 3]);
    let collected: SharedArena<_> = [4, 5, 6].into_iter().collect();

    assert_eq!(extended.iter().copied().collect::<Vec<_>>(), vec![1, 2, 3]);
    assert_eq!(collected.into_iter().collect::<Vec<_>>(), vec![4, 5, 6]);
}

#[test]
fn concurrent_single_allocations_return_unique_valid_capabilities() {
    const THREADS: usize = 8;
    const PER_THREAD: usize = 500;

    let arena = Arc::new(SharedArena::new());
    let mut handles = Vec::with_capacity(THREADS);
    for thread_id in 0..THREADS {
        let arena = Arc::clone(&arena);
        handles.push(thread::spawn(move || {
            (0..PER_THREAD)
                .map(|offset| {
                    let value = thread_id * PER_THREAD + offset;
                    (arena.alloc(value), value)
                })
                .collect::<Vec<_>>()
        }));
    }
    let pairs: Vec<_> = handles
        .into_iter()
        .flat_map(|handle| handle.join().unwrap())
        .collect();

    assert_eq!(arena.len(), THREADS * PER_THREAD);
    for &(idx, expected) in &pairs {
        assert_eq!(*arena.get(idx), expected);
    }
    let mut slots: Vec<_> = pairs.iter().map(|(idx, _)| idx.slot()).collect();
    slots.sort_unstable();
    slots.dedup();
    assert_eq!(slots.len(), THREADS * PER_THREAD);
}

#[test]
fn concurrent_blocks_never_interleave() {
    const THREADS: usize = 12;
    const BLOCK_LEN: usize = 32;

    let arena = Arc::new(SharedArena::new());
    let barrier = Arc::new(Barrier::new(THREADS));
    let mut handles = Vec::with_capacity(THREADS);
    for thread_id in 0..THREADS {
        let arena = Arc::clone(&arena);
        let barrier = Arc::clone(&barrier);
        handles.push(thread::spawn(move || {
            barrier.wait();
            let values = (0..BLOCK_LEN).map(|offset| thread_id * 1000 + offset);
            (thread_id, arena.alloc_block(values))
        }));
    }
    let blocks: Vec<_> = handles
        .into_iter()
        .map(|handle| handle.join().unwrap())
        .collect();

    let mut all_slots = Vec::with_capacity(THREADS * BLOCK_LEN);
    for (thread_id, block) in blocks {
        let indices: Vec<_> = block.indices().collect();
        assert_eq!(indices.len(), BLOCK_LEN);
        assert!(indices.windows(2).all(|pair| {
            pair[0].stamp() == pair[1].stamp() && pair[0].slot() + 1 == pair[1].slot()
        }));
        for (offset, idx) in indices.into_iter().enumerate() {
            assert_eq!(*arena.get(idx), thread_id * 1000 + offset);
            all_slots.push(idx.slot());
        }
    }
    all_slots.sort_unstable();
    all_slots.dedup();
    assert_eq!(all_slots.len(), THREADS * BLOCK_LEN);
}

#[test]
fn readers_observe_only_complete_publication_prefixes() {
    const WRITERS: usize = 4;
    const PER_WRITER: usize = 300;

    let arena = Arc::new(SharedArena::new());
    let done = Arc::new(AtomicBool::new(false));
    let reader_started = Arc::new(Barrier::new(2));
    let reader_arena = Arc::clone(&arena);
    let reader_done = Arc::clone(&done);
    let reader_start = Arc::clone(&reader_started);
    let reader = thread::spawn(move || {
        let mut snapshots = 0;
        loop {
            for (idx, value) in reader_arena.iter_indexed() {
                assert!(reader_arena.is_valid(idx));
                assert_eq!(reader_arena.get(idx), value);
            }
            snapshots += 1;
            if snapshots == 1 {
                reader_start.wait();
            }
            if reader_done.load(Ordering::Acquire) {
                break;
            }
            thread::yield_now();
        }
        snapshots
    });
    // Do not let an unlucky scheduler run every writer to completion before
    // the reader has executed even one publication snapshot.
    reader_started.wait();

    let writers: Vec<_> = (0..WRITERS)
        .map(|thread_id| {
            let arena = Arc::clone(&arena);
            thread::spawn(move || {
                for offset in 0..PER_WRITER {
                    arena.alloc(thread_id * PER_WRITER + offset);
                }
            })
        })
        .collect();
    for writer in writers {
        writer.join().unwrap();
    }
    done.store(true, Ordering::Release);

    assert!(reader.join().unwrap() >= 2);
    assert_eq!(arena.len(), WRITERS * PER_WRITER);
}

#[test]
fn checkpoints_taken_during_allocation_are_monotone_and_readable() {
    let arena = Arc::new(SharedArena::new());
    let done = Arc::new(AtomicBool::new(false));
    let writer_arena = Arc::clone(&arena);
    let writer_done = Arc::clone(&done);
    let writer = thread::spawn(move || {
        for value in 0..2000 {
            writer_arena.alloc(value);
        }
        writer_done.store(true, Ordering::Release);
    });

    let mut lengths = Vec::new();
    while !done.load(Ordering::Acquire) {
        let checkpoint = arena.checkpoint();
        assert_eq!(
            arena.iter_indexed().take(checkpoint.len()).count(),
            checkpoint.len()
        );
        lengths.push(checkpoint.len());
        thread::yield_now();
    }
    writer.join().unwrap();

    assert!(lengths.windows(2).all(|window| window[0] <= window[1]));
    assert_eq!(arena.len(), 2000);
}

#[test]
fn high_contention_completes_without_observed_livelock() {
    const THREADS: usize = 16;
    const PER_THREAD: usize = 250;
    const LIMIT: Duration = Duration::from_secs(10);

    let arena = Arc::new(SharedArena::new());
    let barrier = Arc::new(Barrier::new(THREADS));
    let (done_tx, done_rx) = mpsc::channel();
    let start = Instant::now();
    let handles: Vec<_> = (0..THREADS)
        .map(|thread_id| {
            let arena = Arc::clone(&arena);
            let barrier = Arc::clone(&barrier);
            let done_tx = done_tx.clone();
            thread::spawn(move || {
                barrier.wait();
                for offset in 0..PER_THREAD {
                    arena.alloc(thread_id * PER_THREAD + offset);
                }
                // Signal completion instead of joining directly: the
                // receiver below enforces a shared time budget, so a real
                // hang fails this test with a timeout instead of blocking
                // forever in `join`. Ignore a failed send: it only happens
                // when the main thread already hit the shared deadline,
                // failed its assert, and dropped `done_rx` while unwinding —
                // a slow-but-live worker landing here afterward should not
                // itself panic on the way out and clutter that failure's
                // output.
                let _ = done_tx.send(());
            })
        })
        .collect();
    drop(done_tx);

    // Wait for all THREADS completion signals against one shared deadline,
    // recomputing the remaining budget from wall-clock time on every
    // iteration so the total wait cannot exceed LIMIT regardless of how many
    // signals are still pending (no race on a remainder counter).
    let deadline = start + LIMIT;
    for signaled in 0..THREADS {
        let remaining = deadline.saturating_duration_since(Instant::now());
        assert!(
            done_rx.recv_timeout(remaining).is_ok(),
            "high-contention workers did not finish within {LIMIT:?} or a \
             worker panicked: {signaled} of {THREADS} signaled"
        );
    }
    let signaled_elapsed = start.elapsed();

    // Every worker has signaled completion, so joining cannot block.
    for handle in handles {
        handle.join().unwrap();
    }

    assert_eq!(arena.len(), THREADS * PER_THREAD);
    assert!(
        signaled_elapsed < LIMIT,
        "all workers signaled, but only at the deadline boundary: \
         {signaled_elapsed:?} >= {LIMIT:?}"
    );
}

#[test]
fn rollback_with_a_panicking_drop_keeps_counters_and_is_retryable() {
    struct Tracked {
        drops: Rc<Cell<u32>>,
        panic_on_drop: bool,
    }

    impl Drop for Tracked {
        fn drop(&mut self) {
            self.drops.set(self.drops.get() + 1);
            assert!(!self.panic_on_drop, "intentional panic in Drop");
        }
    }

    let drops = Rc::new(Cell::new(0));
    let mut arena = SharedArena::new();
    let kept = arena.alloc(Tracked {
        drops: Rc::clone(&drops),
        panic_on_drop: false,
    });
    let checkpoint = arena.checkpoint();
    let panicking = arena.alloc(Tracked {
        drops: Rc::clone(&drops),
        panic_on_drop: true,
    });
    let last = arena.alloc(Tracked {
        drops: Rc::clone(&drops),
        panic_on_drop: false,
    });

    let result =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| arena.rollback(checkpoint)));
    assert!(result.is_err());
    assert_eq!(drops.get(), 2);
    assert_eq!(arena.len(), 1);
    assert!(arena.is_valid(kept));
    assert!(!arena.is_valid(panicking));
    assert!(!arena.is_valid(last));
    assert_eq!(
        arena.iter().count(),
        1,
        "iter must not observe an empty published slot"
    );

    assert_eq!(
        arena.try_rollback(checkpoint),
        Ok(()),
        "retry after the partial rollback"
    );
    assert_eq!(arena.len(), 1);
    let replacement = arena.alloc(Tracked {
        drops: Rc::clone(&drops),
        panic_on_drop: false,
    });
    assert_eq!(
        replacement.slot(),
        1,
        "no reservation leaked from the interrupted rollback"
    );
    assert!(arena.is_valid(replacement));
    assert!(!arena.is_valid(panicking));

    drop(arena);
    assert_eq!(
        drops.get(),
        4,
        "kept and replacement are dropped exactly once when the arena drops"
    );
}

#[test]
fn reset_with_a_panicking_drop_keeps_counters_and_is_retryable() {
    struct Tracked {
        drops: Rc<Cell<u32>>,
        panic_on_drop: bool,
    }

    impl Drop for Tracked {
        fn drop(&mut self) {
            self.drops.set(self.drops.get() + 1);
            assert!(!self.panic_on_drop, "intentional panic in Drop");
        }
    }

    let drops = Rc::new(Cell::new(0));
    let mut arena = SharedArena::new();
    let kept = arena.alloc(Tracked {
        drops: Rc::clone(&drops),
        panic_on_drop: false,
    });
    let panicking = arena.alloc(Tracked {
        drops: Rc::clone(&drops),
        panic_on_drop: true,
    });
    arena.alloc(Tracked {
        drops: Rc::clone(&drops),
        panic_on_drop: false,
    });

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| arena.reset()));
    assert!(result.is_err());
    assert_eq!(drops.get(), 2);
    assert_eq!(arena.len(), 1);
    // `reset` always targets length 0, so its segment boundary commits
    // before the drop loop covers *every* existing slot, including `kept`
    // (not yet taken when `panicking`'s destructor fired). See
    // `crate::segments`'s module documentation.
    assert!(!arena.is_valid(kept));
    assert!(!arena.is_valid(panicking));

    arena.reset();
    assert_eq!(drops.get(), 3);
    assert!(arena.is_empty());

    let replacement = arena.alloc(Tracked {
        drops: Rc::clone(&drops),
        panic_on_drop: false,
    });
    assert_eq!(replacement.slot(), 0);
    assert!(arena.is_valid(replacement));
    assert!(!arena.is_valid(kept));
}

// `drain` collects every value into a local `Vec` and already reports
// `len() == 0` by the time it returns normally: unlike `rollback`/`reset`,
// there is no partial-drain state for a value's destructor to interrupt
// here. A destructor that panics on drop only fires later, when the caller
// drops the returned (but never consumed) iterator. This checks that every
// value still drops exactly once when that happens, and that the arena —
// already empty before the panic — stays usable afterward.
#[test]
fn drain_drops_every_value_exactly_once_when_the_returned_iterator_panics_on_drop() {
    struct Tracked {
        drops: Rc<Cell<u32>>,
        panic_on_drop: bool,
    }

    impl Drop for Tracked {
        fn drop(&mut self) {
            self.drops.set(self.drops.get() + 1);
            assert!(!self.panic_on_drop, "intentional panic in Drop");
        }
    }

    let drops = Rc::new(Cell::new(0));
    let mut arena = SharedArena::new();
    arena.alloc(Tracked {
        drops: Rc::clone(&drops),
        panic_on_drop: true,
    });
    arena.alloc(Tracked {
        drops: Rc::clone(&drops),
        panic_on_drop: false,
    });

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _drained = arena.drain();
    }));
    assert!(result.is_err());
    assert_eq!(arena.len(), 0);
    assert_eq!(drops.get(), 2, "both values drop exactly once");

    let replacement = arena.alloc(Tracked {
        drops: Rc::clone(&drops),
        panic_on_drop: false,
    });
    assert_eq!(
        replacement.slot(),
        0,
        "the arena is reusable after the panic"
    );
    assert!(arena.is_valid(replacement));
}

#[test]
fn checkpoint_taken_before_drain_is_rejected_then_reported_as_diverged() {
    let mut arena = SharedArena::new();
    arena.alloc(1);
    arena.alloc(2);
    let checkpoint = arena.checkpoint();
    let _ = arena.drain().count();

    assert_eq!(
        arena.try_rollback(checkpoint),
        Err(CheckpointError::BeyondCurrent {
            checkpoint_len: 2,
            current_len: 0,
        })
    );

    arena.alloc(3);
    arena.alloc(4);
    assert_eq!(
        arena.try_rollback(checkpoint),
        Err(CheckpointError::DivergedPrefix { checkpoint_len: 2 })
    );
    assert_eq!(arena.len(), 2);
}

#[test]
fn checkpoint_from_the_other_arena_type_is_rejected_as_foreign_in_both_directions() {
    let single: Arena<i32> = Arena::new();
    let single_checkpoint = single.checkpoint();
    let mut shared = SharedArena::new();
    shared.alloc(1);
    assert_eq!(
        shared.try_rollback(single_checkpoint),
        Err(CheckpointError::ForeignArena)
    );
    assert_eq!(shared.len(), 1);

    let shared_checkpoint = shared.checkpoint();
    let mut other_single = Arena::new();
    other_single.alloc(1);
    assert_eq!(
        other_single.try_rollback(shared_checkpoint),
        Err(CheckpointError::ForeignArena)
    );
    assert_eq!(other_single.len(), 1);
}

#[test]
fn rollback_then_alloc_block_reuses_slots_without_tripping_the_occupied_assertion() {
    let mut arena = SharedArena::new();
    let root = arena.alloc(0);
    let checkpoint = arena.checkpoint();
    let old_block = arena.alloc_block([1, 2, 3]);
    arena.rollback(checkpoint);
    let new_block = arena.alloc_block([4, 5, 6]);

    assert_eq!(new_block.first().map(Idx::slot), Some(1));
    assert!(arena.is_valid(root));
    assert!(old_block.indices().all(|idx| !arena.is_valid(idx)));
    assert!(new_block.indices().all(|idx| arena.is_valid(idx)));
    assert_eq!(arena.len(), 4);
    assert!(!old_block.contains(new_block.get(0).unwrap()));
}

#[test]
fn shared_arena_creation_inside_a_tls_destructor_at_thread_exit_works() {
    use std::sync::atomic::{AtomicBool, Ordering};

    static OK: AtomicBool = AtomicBool::new(false);

    struct OnExit;

    impl Drop for OnExit {
        fn drop(&mut self) {
            let arena = SharedArena::new();
            let idx = arena.alloc(1);
            if arena.is_valid(idx) {
                OK.store(true, Ordering::SeqCst);
            }
        }
    }

    thread_local! {
        static ON_EXIT: OnExit = const { OnExit };
    }
    std::thread::spawn(|| ON_EXIT.with(|_| {})).join().unwrap();

    assert!(OK.load(Ordering::SeqCst));
}
