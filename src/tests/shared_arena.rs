use std::cell::Cell;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Barrier};
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
            pair[0].same_allocation(pair[1]) && pair[0].slot() + 1 == pair[1].slot()
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
    let start = Instant::now();
    let handles: Vec<_> = (0..THREADS)
        .map(|thread_id| {
            let arena = Arc::clone(&arena);
            let barrier = Arc::clone(&barrier);
            thread::spawn(move || {
                barrier.wait();
                for offset in 0..PER_THREAD {
                    arena.alloc(thread_id * PER_THREAD + offset);
                }
            })
        })
        .collect();
    for handle in handles {
        handle.join().unwrap();
    }

    assert_eq!(arena.len(), THREADS * PER_THREAD);
    assert!(start.elapsed() < LIMIT);
}
