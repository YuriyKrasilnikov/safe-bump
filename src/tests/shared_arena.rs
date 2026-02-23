use std::cell::Cell;
use std::rc::Rc;

use super::*;

// ─── Compile-time guarantees ────────────────────────────────────────────────

#[test]
fn is_send_and_sync() {
    fn assert_send<T: Send>() {}
    fn assert_sync<T: Sync>() {}

    assert_send::<SharedArena<String>>();
    assert_sync::<SharedArena<String>>();

    assert_send::<SharedArena<i32>>();
    assert_sync::<SharedArena<i32>>();
}

#[test]
fn alloc_takes_shared_ref() {
    // SharedArena::alloc(&self, T) — not &mut self
    // This compiles only if alloc takes &self.
    let arena = SharedArena::<i32>::new();
    let _a = arena.alloc(1);
    let _b = arena.alloc(2); // second alloc without &mut — proves &self
}

#[test]
fn get_returns_ref_not_guard() {
    let arena = SharedArena::<i32>::new();
    let a = arena.alloc(42);

    // Type annotation proves get returns &T, not a guard type.
    let r: &i32 = arena.get(a);
    assert_eq!(*r, 42);
}

// ─── Basic operations ───────────────────────────────────────────────────────

#[test]
fn alloc_and_get() {
    let arena = SharedArena::new();
    let a = arena.alloc(42);
    let b = arena.alloc(99);

    assert_eq!(*arena.get(a), 42);
    assert_eq!(*arena.get(b), 99);
    assert_eq!(arena.len(), 2);
}

#[test]
fn alloc_strings() {
    let arena = SharedArena::new();
    let a = arena.alloc(String::from("hello"));
    let b = arena.alloc(String::from("world"));

    assert_eq!(arena[a], "hello");
    assert_eq!(arena[b], "world");
}

#[test]
fn empty_arena() {
    let arena = SharedArena::<i32>::new();
    assert!(arena.is_empty());
    assert_eq!(arena.len(), 0);
}

#[test]
fn index_operator() {
    let arena = SharedArena::new();
    let a = arena.alloc(String::from("hello"));

    assert_eq!(arena[a], "hello");
}

#[test]
fn is_valid_in_range() {
    let arena = SharedArena::new();
    let a = arena.alloc(1);

    assert!(arena.is_valid(a));
    assert!(!arena.is_valid(Idx::from_raw(999)));
}

#[test]
fn try_get_valid_and_invalid() {
    let arena = SharedArena::new();
    let a = arena.alloc(42);

    assert_eq!(arena.try_get(a), Some(&42));
    assert_eq!(arena.try_get(Idx::from_raw(999)), None);
}

#[test]
fn many_allocations() {
    let arena = SharedArena::new();
    for i in 0..10_000_u64 {
        let idx = arena.alloc(i);
        assert_eq!(*arena.get(idx), i);
    }
    assert_eq!(arena.len(), 10_000);
}

// ─── Checkpoint/rollback ────────────────────────────────────────────────────

#[test]
fn checkpoint_rollback() {
    let mut arena = SharedArena::new();
    let a = arena.alloc(1);
    let cp = arena.checkpoint();
    let _b = arena.alloc(2);
    let _c = arena.alloc(3);

    arena.rollback(cp);
    assert_eq!(arena.len(), 1);
    assert_eq!(*arena.get(a), 1);
}

#[test]
fn checkpoint_len() {
    let arena = SharedArena::new();
    arena.alloc(1);
    arena.alloc(2);
    let cp = arena.checkpoint();
    assert_eq!(cp.len(), 2);
}

#[test]
fn checkpoint_keep() {
    let arena = SharedArena::new();
    let a = arena.alloc(1);
    let _cp = arena.checkpoint();

    // Allocate speculatively
    let b = arena.alloc(2);
    let c = arena.alloc(3);

    // Decide to KEEP — simply don't rollback
    assert_eq!(arena.len(), 3);
    assert_eq!(*arena.get(a), 1);
    assert_eq!(*arena.get(b), 2);
    assert_eq!(*arena.get(c), 3);
}

#[test]
fn rollback_to_empty() {
    let mut arena = SharedArena::new();
    let cp = arena.checkpoint();

    arena.alloc(1);
    arena.alloc(2);
    arena.rollback(cp);

    assert!(arena.is_empty());
}

#[test]
#[should_panic(expected = "checkpoint")]
fn rollback_beyond_length_panics() {
    let mut arena = SharedArena::new();
    arena.alloc(1);
    arena.alloc(2);
    let cp_early = arena.checkpoint(); // saves len=2
    arena.alloc(3);
    arena.alloc(4);
    arena.alloc(5);
    let cp_late = arena.checkpoint(); // saves len=5
    arena.rollback(cp_early); // back to len=2
    arena.rollback(cp_late); // panics: checkpoint beyond current length
}

#[test]
fn nested_checkpoints() {
    let mut arena = SharedArena::new();
    let a = arena.alloc(1);

    let cp1 = arena.checkpoint();
    let _b = arena.alloc(2);

    let cp2 = arena.checkpoint();
    let _c = arena.alloc(3);

    arena.rollback(cp2);
    assert_eq!(arena.len(), 2);

    arena.rollback(cp1);
    assert_eq!(arena.len(), 1);
    assert_eq!(*arena.get(a), 1);
}

// ─── Drop semantics ────────────────────────────────────────────────────────

#[test]
fn rollback_runs_drop() {
    let drop_count = Rc::new(Cell::new(0u32));
    let mut arena = SharedArena::new();
    let _a = arena.alloc(Tracked(Rc::clone(&drop_count)));
    let cp = arena.checkpoint();
    let _b = arena.alloc(Tracked(Rc::clone(&drop_count)));
    let _c = arena.alloc(Tracked(Rc::clone(&drop_count)));

    assert_eq!(drop_count.get(), 0);
    arena.rollback(cp);
    assert_eq!(drop_count.get(), 2); // b and c dropped
}

#[test]
fn reset_runs_drop() {
    let drop_count = Rc::new(Cell::new(0u32));
    let mut arena = SharedArena::new();
    let _a = arena.alloc(Tracked(Rc::clone(&drop_count)));
    let _b = arena.alloc(Tracked(Rc::clone(&drop_count)));

    arena.reset();
    assert_eq!(drop_count.get(), 2);
    assert!(arena.is_empty());
}

#[test]
fn drop_arena_runs_drop() {
    let drop_count = Rc::new(Cell::new(0u32));

    {
        let arena = SharedArena::new();
        arena.alloc(Tracked(Rc::clone(&drop_count)));
        arena.alloc(Tracked(Rc::clone(&drop_count)));
        arena.alloc(Tracked(Rc::clone(&drop_count)));
        assert_eq!(drop_count.get(), 0);
    } // arena dropped here

    assert_eq!(drop_count.get(), 3);
}

// ─── Stale index / validity ─────────────────────────────────────────────────

#[test]
#[should_panic(expected = "index out of bounds")]
fn stale_index_panics() {
    let mut arena = SharedArena::new();
    let _a = arena.alloc(1);
    let b = arena.alloc(2);

    arena.reset();
    let _ = arena[b]; // stale index
}

#[test]
fn is_valid_after_rollback() {
    let mut arena = SharedArena::new();
    let a = arena.alloc(1);
    let cp = arena.checkpoint();
    let b = arena.alloc(2);

    assert!(arena.is_valid(a));
    assert!(arena.is_valid(b));

    arena.rollback(cp);
    assert!(arena.is_valid(a));
    assert!(!arena.is_valid(b));
}

#[test]
fn is_valid_after_reset() {
    let mut arena = SharedArena::new();
    let a = arena.alloc(1);

    assert!(arena.is_valid(a));
    arena.reset();
    assert!(!arena.is_valid(a));
}

#[test]
fn try_get_after_rollback() {
    let mut arena = SharedArena::new();
    let a = arena.alloc(42);
    let cp = arena.checkpoint();
    let b = arena.alloc(99);

    arena.rollback(cp);
    assert_eq!(arena.try_get(a), Some(&42));
    assert_eq!(arena.try_get(b), None);
}

// ─── Reuse after reset ──────────────────────────────────────────────────────

#[test]
fn reuse_after_reset() {
    let mut arena = SharedArena::new();
    arena.alloc(String::from("first"));
    arena.reset();

    let a = arena.alloc(String::from("second"));
    assert_eq!(arena[a], "second");
    assert_eq!(arena.len(), 1);
}

// ─── Batch allocation ───────────────────────────────────────────────────────

#[test]
fn alloc_extend_returns_first_idx() {
    let arena = SharedArena::new();
    arena.alloc(0);

    let first = arena.alloc_extend(vec![10, 20, 30]);
    assert_eq!(first, Some(Idx::from_raw(1)));
    assert_eq!(arena.len(), 4);
    assert_eq!(*arena.get(Idx::from_raw(1)), 10);
    assert_eq!(*arena.get(Idx::from_raw(2)), 20);
    assert_eq!(*arena.get(Idx::from_raw(3)), 30);
}

#[test]
fn alloc_extend_empty_returns_none() {
    let arena = SharedArena::<i32>::new();
    let result = arena.alloc_extend(std::iter::empty());
    assert_eq!(result, None);
    assert!(arena.is_empty());
}

// ─── Drain / IntoIterator ───────────────────────────────────────────────────

#[test]
fn drain_returns_all_items() {
    let mut arena = SharedArena::new();
    arena.alloc(10);
    arena.alloc(20);
    arena.alloc(30);

    let items: Vec<_> = arena.drain().collect();
    assert_eq!(items, vec![10, 20, 30]);
    assert!(arena.is_empty());
}

#[test]
fn drain_runs_no_extra_drops() {
    let drop_count = Rc::new(Cell::new(0u32));
    let mut arena = SharedArena::new();
    arena.alloc(Tracked(Rc::clone(&drop_count)));
    arena.alloc(Tracked(Rc::clone(&drop_count)));

    let items: Vec<_> = arena.drain().collect();
    assert_eq!(drop_count.get(), 0); // not dropped yet — owned by items
    drop(items);
    assert_eq!(drop_count.get(), 2); // now dropped
}

#[test]
fn into_iter_consuming() {
    let arena = SharedArena::new();
    arena.alloc(String::from("a"));
    arena.alloc(String::from("b"));
    arena.alloc(String::from("c"));

    let collected: Vec<String> = arena.into_iter().collect();
    assert_eq!(collected, vec!["a", "b", "c"]);
}

// ─── Concurrent allocation ──────────────────────────────────────────────────

#[test]
fn concurrent_alloc() {
    use std::sync::Arc;
    use std::thread;

    let arena = Arc::new(SharedArena::<u64>::new());

    let indices: Vec<Idx<u64>> = (0..4)
        .map(|i| {
            let arena = Arc::clone(&arena);
            thread::spawn(move || arena.alloc(i))
        })
        .collect::<Vec<_>>()
        .into_iter()
        .map(|h| h.join().unwrap())
        .collect();

    // All values accessible and valid
    for idx in &indices {
        assert!(arena.is_valid(*idx));
    }
    assert_eq!(arena.len(), 4);
}

#[test]
fn concurrent_alloc_and_read() {
    use std::sync::Arc;
    use std::thread;

    let arena = Arc::new(SharedArena::<u64>::new());

    // Pre-allocate some values
    let pre = arena.alloc(100);

    // Concurrent readers + writers
    let mut handles = Vec::new();

    // Writers
    for i in 0..4 {
        let arena = Arc::clone(&arena);
        handles.push(thread::spawn(move || {
            arena.alloc(i);
        }));
    }

    // Readers (reading pre-allocated value while writers are active)
    for _ in 0..4 {
        let arena = Arc::clone(&arena);
        handles.push(thread::spawn(move || {
            assert_eq!(*arena.get(pre), 100);
        }));
    }

    for h in handles {
        h.join().unwrap();
    }

    assert_eq!(arena.len(), 5); // 1 pre + 4 concurrent
}

// ─── Stress tests ───────────────────────────────────────────────────────────

#[test]
fn stress_many_threads_many_allocs() {
    use std::sync::Arc;
    use std::thread;

    const THREADS: usize = 8;
    const PER_THREAD: usize = 1000;

    let arena = Arc::new(SharedArena::<usize>::new());

    let all_pairs: Vec<(Idx<usize>, usize)> = (0..THREADS)
        .map(|t| {
            let arena = Arc::clone(&arena);
            thread::spawn(move || {
                let mut indices = Vec::with_capacity(PER_THREAD);
                for i in 0..PER_THREAD {
                    let val = t * PER_THREAD + i;
                    indices.push((arena.alloc(val), val));
                }
                indices
            })
        })
        .collect::<Vec<_>>()
        .into_iter()
        .flat_map(|h| h.join().unwrap())
        .collect();

    // All values published
    assert_eq!(arena.len(), THREADS * PER_THREAD);

    // Every value accessible and correct
    for (idx, expected) in &all_pairs {
        assert_eq!(*arena.get(*idx), *expected);
    }

    // No duplicates in indices
    let mut raw_indices: Vec<usize> = all_pairs.iter().map(|(idx, _)| idx.into_raw()).collect();
    raw_indices.sort_unstable();
    raw_indices.dedup();
    assert_eq!(raw_indices.len(), THREADS * PER_THREAD);
}

#[test]
fn stress_concurrent_alloc_with_barrier() {
    use std::sync::{Arc, Barrier};
    use std::thread;

    const THREADS: usize = 8;

    let arena = Arc::new(SharedArena::<usize>::new());
    let barrier = Arc::new(Barrier::new(THREADS));

    let indices: Vec<Idx<usize>> = (0..THREADS)
        .map(|t| {
            let arena = Arc::clone(&arena);
            let barrier = Arc::clone(&barrier);
            thread::spawn(move || {
                // All threads start simultaneously
                barrier.wait();
                arena.alloc(t)
            })
        })
        .collect::<Vec<_>>()
        .into_iter()
        .map(|h| h.join().unwrap())
        .collect();

    assert_eq!(arena.len(), THREADS);

    // All values present (order may vary)
    let mut values: Vec<usize> = indices.iter().map(|idx| *arena.get(*idx)).collect();
    values.sort_unstable();
    assert_eq!(values, (0..THREADS).collect::<Vec<_>>());
}

#[test]
fn stress_published_consistency() {
    use std::sync::Arc;
    use std::sync::Barrier;
    use std::sync::atomic::{AtomicBool, Ordering as AOrdering};
    use std::thread;

    let arena = Arc::new(SharedArena::<u64>::new());
    let done = Arc::new(AtomicBool::new(false));
    // Barrier ensures reader is running before writers start
    let barrier = Arc::new(Barrier::new(5)); // 1 reader + 4 writers

    // Reader thread: continuously checks that all idx < len() are readable
    let reader_arena = Arc::clone(&arena);
    let reader_done = Arc::clone(&done);
    let reader_barrier = Arc::clone(&barrier);
    let reader = thread::spawn(move || {
        reader_barrier.wait();
        let mut checks = 0u64;
        while !reader_done.load(AOrdering::Relaxed) {
            let len = reader_arena.len();
            // Every index below published must be readable
            for i in 0..len {
                let idx = Idx::from_raw(i);
                assert!(reader_arena.is_valid(idx));
                // get must not panic
                let _ = reader_arena.get(idx);
            }
            checks += 1;
        }
        checks
    });

    // Writers: allocate concurrently
    let writers: Vec<_> = (0..4)
        .map(|_| {
            let arena = Arc::clone(&arena);
            let barrier = Arc::clone(&barrier);
            thread::spawn(move || {
                barrier.wait();
                for i in 0..500_u64 {
                    arena.alloc(i);
                }
            })
        })
        .collect();

    for w in writers {
        w.join().unwrap();
    }

    done.store(true, AOrdering::Relaxed);
    let checks = reader.join().unwrap();

    assert_eq!(arena.len(), 2000);
    assert!(checks > 0, "reader performed {checks} checks");
}

#[test]
fn stress_no_livelock() {
    use std::sync::{Arc, Barrier};
    use std::thread;
    use std::time::{Duration, Instant};

    // Many threads start simultaneously — maximizes contention in
    // advance_published where each thread spins waiting for its
    // predecessor to publish.
    const THREADS: usize = 16;
    const PER_THREAD: usize = 500;
    const TIMEOUT: Duration = Duration::from_secs(10);

    let arena = Arc::new(SharedArena::<usize>::new());
    let barrier = Arc::new(Barrier::new(THREADS));
    let start = Instant::now();

    let handles: Vec<_> = (0..THREADS)
        .map(|t| {
            let arena = Arc::clone(&arena);
            let barrier = Arc::clone(&barrier);
            thread::spawn(move || {
                barrier.wait();
                for i in 0..PER_THREAD {
                    arena.alloc(t * PER_THREAD + i);
                }
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }

    let elapsed = start.elapsed();
    assert_eq!(arena.len(), THREADS * PER_THREAD);
    assert!(
        elapsed < TIMEOUT,
        "completed in {elapsed:?} — exceeds {TIMEOUT:?}, possible livelock"
    );
}

// ─── Publish ordering ───────────────────────────────────────────────────────

#[test]
fn published_is_contiguous_prefix() {
    // Verifies that `published` always represents a contiguous prefix
    // of filled slots — no gaps. A reader seeing len() = N can safely
    // read all indices 0..N.
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering as AOrdering};
    use std::thread;

    let arena = Arc::new(SharedArena::<u64>::new());
    let done = Arc::new(AtomicBool::new(false));
    let gap_found = Arc::new(AtomicBool::new(false));
    let max_checked = Arc::new(AtomicUsize::new(0));

    // Reader: for every observed len(), verify ALL slots 0..len() are readable
    let r_arena = Arc::clone(&arena);
    let r_done = Arc::clone(&done);
    let r_gap = Arc::clone(&gap_found);
    let r_max = Arc::clone(&max_checked);
    let reader = thread::spawn(move || {
        while !r_done.load(AOrdering::Relaxed) {
            let len = r_arena.len();
            for i in 0..len {
                if r_arena.try_get(Idx::from_raw(i)).is_none() {
                    r_gap.store(true, AOrdering::Relaxed);
                    return;
                }
            }
            r_max.fetch_max(len, AOrdering::Relaxed);
        }
    });

    // Writers: 8 threads, 500 allocs each, maximum contention
    let writers: Vec<_> = (0..8)
        .map(|_| {
            let arena = Arc::clone(&arena);
            thread::spawn(move || {
                for i in 0..500_u64 {
                    arena.alloc(i);
                }
            })
        })
        .collect();

    for w in writers {
        w.join().unwrap();
    }
    done.store(true, AOrdering::Relaxed);
    reader.join().unwrap();

    assert!(
        !gap_found.load(AOrdering::Relaxed),
        "gap detected in published prefix"
    );
    assert!(
        max_checked.load(AOrdering::Relaxed) > 0,
        "reader performed no checks"
    );
    assert_eq!(arena.len(), 4000);
}

#[test]
fn checkpoint_during_concurrent_alloc() {
    // Checkpoint taken while other threads are allocating.
    // The checkpoint captures a consistent published snapshot —
    // all indices below checkpoint.len() must be readable.
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering as AOrdering};
    use std::thread;

    const PER_WRITER: usize = 500;
    const WRITERS: usize = 4;

    let arena = Arc::new(SharedArena::<u64>::new());
    let writers_done = Arc::new(AtomicBool::new(false));

    // Writers: fixed allocation count (bounded memory)
    let writers: Vec<_> = (0..WRITERS)
        .map(|_| {
            let arena = Arc::clone(&arena);
            thread::spawn(move || {
                for i in 0..PER_WRITER as u64 {
                    arena.alloc(i);
                }
            })
        })
        .collect();

    // Checker: takes checkpoints concurrently with writers
    let cp_arena = Arc::clone(&arena);
    let cp_done = Arc::clone(&writers_done);
    let checker = thread::spawn(move || {
        let mut checkpoints = Vec::new();
        while !cp_done.load(AOrdering::Relaxed) {
            let cp = cp_arena.checkpoint();
            let len = cp.len();
            // Every index below checkpoint must be readable NOW
            for i in 0..len {
                assert!(
                    cp_arena.try_get(Idx::from_raw(i)).is_some(),
                    "checkpoint len={len} but index {i} not readable"
                );
            }
            checkpoints.push(cp);
            std::thread::yield_now();
        }
        checkpoints
    });

    for w in writers {
        w.join().unwrap();
    }
    writers_done.store(true, AOrdering::Relaxed);

    let checkpoints = checker.join().unwrap();

    // Checkpoints are monotonically non-decreasing
    for window in checkpoints.windows(2) {
        assert!(window[0].len() <= window[1].len());
    }
    assert_eq!(arena.len(), WRITERS * PER_WRITER);
}

// ─── Default ────────────────────────────────────────────────────────────────

#[test]
fn default_is_empty() {
    let arena: SharedArena<u8> = SharedArena::default();
    assert!(arena.is_empty());
}
