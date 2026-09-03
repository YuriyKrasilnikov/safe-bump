use std::cell::Cell;
use std::rc::Rc;

use super::*;

#[test]
fn empty_arena() {
    let arena: Arena<i32> = Arena::new();
    assert!(arena.is_empty());
    assert_eq!(arena.len(), 0);
}

#[test]
fn alloc_and_access() {
    let mut arena = Arena::new();
    let a = arena.alloc(42);
    let b = arena.alloc(99);

    assert_eq!(arena[a], 42);
    assert_eq!(arena[b], 99);
    assert_eq!(arena.len(), 2);
}

#[test]
fn alloc_strings() {
    let mut arena = Arena::new();
    let a = arena.alloc(String::from("hello"));
    let b = arena.alloc(String::from("world"));

    assert_eq!(arena[a], "hello");
    assert_eq!(arena[b], "world");
}

#[test]
fn get_mut_modifies() {
    let mut arena = Arena::new();
    let a = arena.alloc(String::from("old"));

    arena[a] = String::from("new");
    assert_eq!(arena[a], "new");
}

#[test]
fn with_capacity() {
    let arena: Arena<u64> = Arena::with_capacity(100);
    assert!(arena.capacity() >= 100);
    assert!(arena.is_empty());
}

#[test]
fn checkpoint_rollback() {
    let mut arena = Arena::new();
    let a = arena.alloc(1);
    let b = arena.alloc(2);
    let cp = arena.checkpoint();

    let _c = arena.alloc(3);
    let _d = arena.alloc(4);
    assert_eq!(arena.len(), 4);

    arena.rollback(cp);
    assert_eq!(arena.len(), 2);
    assert_eq!(arena[a], 1);
    assert_eq!(arena[b], 2);
}

#[test]
fn rollback_runs_drop() {
    let drop_count = Rc::new(Cell::new(0u32));
    let mut arena = Arena::new();
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
    let mut arena = Arena::new();
    let _a = arena.alloc(Tracked(Rc::clone(&drop_count)));
    let _b = arena.alloc(Tracked(Rc::clone(&drop_count)));

    arena.reset();
    assert_eq!(drop_count.get(), 2);
    assert!(arena.is_empty());
}

#[test]
fn reset_preserves_capacity() {
    let mut arena = Arena::with_capacity(100);
    for i in 0..50 {
        arena.alloc(i);
    }
    let cap_before = arena.capacity();

    arena.reset();
    assert!(arena.is_empty());
    assert_eq!(arena.capacity(), cap_before);
}

#[test]
fn nested_checkpoints() {
    let mut arena = Arena::new();
    let a = arena.alloc(1);

    let cp1 = arena.checkpoint();
    let _b = arena.alloc(2);

    let cp2 = arena.checkpoint();
    let _c = arena.alloc(3);

    arena.rollback(cp2);
    assert_eq!(arena.len(), 2);

    arena.rollback(cp1);
    assert_eq!(arena.len(), 1);
    assert_eq!(arena[a], 1);
}

#[test]
fn rollback_to_empty() {
    let mut arena = Arena::new();
    let cp = arena.checkpoint();

    arena.alloc(1);
    arena.alloc(2);
    arena.rollback(cp);

    assert!(arena.is_empty());
}

#[test]
#[should_panic(expected = "checkpoint length 5 exceeds current length 2")]
fn rollback_beyond_length_panics() {
    let mut arena = Arena::new();
    arena.alloc(1);
    arena.alloc(2);
    let cp_early = arena.checkpoint(); // saves len=2
    arena.alloc(3);
    arena.alloc(4);
    arena.alloc(5);
    let cp_late = arena.checkpoint(); // saves len=5
    arena.rollback(cp_early); // back to len=2
    arena.rollback(cp_late); // panics: checkpoint 5 > current length 2
}

#[test]
#[should_panic(expected = "foreign or stale")]
fn stale_index_panics() {
    let mut arena = Arena::new();
    let _a = arena.alloc(1);
    let b = arena.alloc(2);

    arena.reset();
    let _ = arena[b]; // stale index
}

#[test]
fn idx_is_copy() {
    let mut arena = Arena::new();
    let a = arena.alloc(42);
    let b = a; // Copy
    assert_eq!(arena[a], arena[b]);
}

#[test]
fn idx_equality() {
    let mut arena = Arena::new();
    let a = arena.alloc(5);
    let b = a;
    let c = arena.alloc(3);

    assert_eq!(a, b);
    assert_ne!(a, c);
}

#[test]
fn idx_ordering() {
    let mut arena = Arena::new();
    let a = arena.alloc(1);
    let b = arena.alloc(5);

    assert!(a < b);
}

#[test]
fn idx_exposes_slot_without_a_reverse_constructor() {
    let mut arena = Arena::new();
    let first = arena.alloc(String::from("first"));
    let second = arena.alloc(String::from("second"));
    assert_eq!(first.slot(), 0);
    assert_eq!(second.slot(), 1);
}

#[test]
fn iter() {
    let mut arena = Arena::new();
    arena.alloc(10);
    arena.alloc(20);
    arena.alloc(30);

    let sum: i32 = arena.iter().sum();
    assert_eq!(sum, 60);
}

#[test]
fn default_is_empty() {
    let arena: Arena<u8> = Arena::default();
    assert!(arena.is_empty());
}

#[test]
fn many_allocations() {
    let mut arena = Arena::with_capacity(0);
    for i in 0..10_000 {
        let idx = arena.alloc(i);
        assert_eq!(arena[idx], i);
    }
    assert_eq!(arena.len(), 10_000);
}

#[test]
fn checkpoint_len() {
    let mut arena = Arena::new();
    arena.alloc(1);
    arena.alloc(2);
    let cp = arena.checkpoint();
    assert_eq!(cp.len(), 2);
}

#[test]
fn reuse_after_reset() {
    let mut arena = Arena::new();
    arena.alloc(String::from("first"));
    arena.reset();

    let a = arena.alloc(String::from("second"));
    assert_eq!(arena[a], "second");
    assert_eq!(arena.len(), 1);
}

#[test]
fn alloc_block_carries_every_contiguous_index() {
    let mut arena = Arena::new();
    arena.alloc(0);

    let block = arena.alloc_block(vec![10, 20, 30]);
    assert_eq!(block.len(), 3);
    assert_eq!(block.first().map(Idx::slot), Some(1));
    assert_eq!(arena.len(), 4);
    assert_eq!(arena[block.get(0).unwrap()], 10);
    assert_eq!(arena[block.get(1).unwrap()], 20);
    assert_eq!(arena[block.get(2).unwrap()], 30);
    assert_eq!(block.get(3), None);
    assert!(block.indices().all(|idx| block.contains(idx)));
}

#[test]
fn alloc_block_empty_returns_an_empty_capability() {
    let mut arena: Arena<i32> = Arena::new();
    let result = arena.alloc_block(std::iter::empty());
    assert!(result.is_empty());
    assert_eq!(result.first(), None);
    assert_eq!(result.indices().count(), 0);
    assert!(arena.is_empty());
}

#[test]
fn is_valid_after_rollback() {
    let mut arena = Arena::new();
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
    let mut arena = Arena::new();
    let a = arena.alloc(1);

    assert!(arena.is_valid(a));
    arena.reset();
    assert!(!arena.is_valid(a));
}

#[test]
fn try_get_returns_none_for_stale() {
    let mut arena = Arena::new();
    let a = arena.alloc(42);
    let cp = arena.checkpoint();
    let b = arena.alloc(99);

    arena.rollback(cp);
    assert_eq!(arena.try_get(a), Some(&42));
    assert_eq!(arena.try_get(b), None);
}

#[test]
fn try_get_mut_returns_none_for_stale() {
    let mut arena = Arena::new();
    let _a = arena.alloc(1);
    let cp = arena.checkpoint();
    let b = arena.alloc(2);

    arena.rollback(cp);
    assert_eq!(arena.try_get_mut(b), None);
}

#[test]
fn drain_returns_all_items() {
    let mut arena = Arena::new();
    arena.alloc(10);
    arena.alloc(20);
    arena.alloc(30);

    let items: Vec<_> = arena.drain().collect();
    assert_eq!(items, vec![10, 20, 30]);
    assert!(arena.is_empty());
}

/// `drain` commits a fresh segment stamp before `Vec::drain` even runs (see
/// `Self::drain`'s doc comment), so slots regrown to the exact pre-drain
/// length belong to a new generation, not the old one: every pre-drain
/// `Idx` is rejected, a pre-drain `Checkpoint` whose length no longer
/// exceeds the regrown length is rejected as `DivergedPrefix` rather than
/// wrongly validating, and only the freshly minted indices are valid.
#[test]
fn drain_then_regrowth_to_the_same_length_rejects_every_pre_drain_capability() {
    let mut arena = Arena::new();
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
/// but the iterator `drain` returns is dropped without being consumed:
/// `Vec::drain` already truncates the arena's value vector to length 0
/// synchronously when `drain` is called, before this test ever drops the
/// returned iterator, so the regrowth below observes the same empty arena
/// either way.
#[test]
fn drain_dropped_without_consuming_still_regrows_under_a_new_generation() {
    let mut arena = Arena::new();
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
fn drain_runs_no_extra_drops() {
    let drop_count = Rc::new(Cell::new(0u32));
    let mut arena = Arena::new();
    arena.alloc(Tracked(Rc::clone(&drop_count)));
    arena.alloc(Tracked(Rc::clone(&drop_count)));

    let items: Vec<_> = arena.drain().collect();
    assert_eq!(drop_count.get(), 0); // not dropped yet — owned by items
    drop(items);
    assert_eq!(drop_count.get(), 2); // now dropped
}

#[test]
fn iter_indexed_yields_correct_pairs() {
    let mut arena = Arena::new();
    let a = arena.alloc("x");
    let b = arena.alloc("y");
    let c = arena.alloc("z");

    let pairs: Vec<_> = arena.iter_indexed().collect();
    assert_eq!(pairs.len(), 3);
    assert_eq!(pairs[0], (a, &"x"));
    assert_eq!(pairs[1], (b, &"y"));
    assert_eq!(pairs[2], (c, &"z"));
}

#[test]
fn iter_indexed_empty() {
    let arena: Arena<i32> = Arena::new();
    assert_eq!(arena.iter_indexed().count(), 0);
}

#[test]
fn iter_indexed_exact_size() {
    let mut arena = Arena::new();
    arena.alloc(1);
    arena.alloc(2);
    arena.alloc(3);

    let iter = arena.iter_indexed();
    assert_eq!(iter.len(), 3);
}

#[test]
fn shrink_to_fit_reduces_capacity() {
    let mut arena: Arena<u64> = Arena::with_capacity(1000);
    arena.alloc(1);
    arena.alloc(2);
    assert!(arena.capacity() >= 1000);

    arena.shrink_to_fit();
    assert!(arena.capacity() < 1000);
    assert_eq!(arena.len(), 2);
}

#[test]
fn iter_mut_modifies_all() {
    let mut arena = Arena::new();
    arena.alloc(1);
    arena.alloc(2);
    arena.alloc(3);

    for item in &mut arena {
        *item *= 10;
    }

    let values: Vec<_> = arena.iter().copied().collect();
    assert_eq!(values, vec![10, 20, 30]);
}

#[test]
fn iter_indexed_mut_yields_correct_pairs() {
    let mut arena = Arena::new();
    let a = arena.alloc(String::from("x"));
    let b = arena.alloc(String::from("y"));

    let pairs: Vec<_> = arena
        .iter_indexed_mut()
        .map(|(idx, val)| (idx, val.clone()))
        .collect();
    assert_eq!(pairs.len(), 2);
    assert_eq!(pairs[0], (a, String::from("x")));
    assert_eq!(pairs[1], (b, String::from("y")));
}

#[test]
fn iter_indexed_mut_modifies() {
    let mut arena = Arena::new();
    let first = arena.alloc(1);
    let second = arena.alloc(2);
    let third = arena.alloc(3);

    for (_, val) in arena.iter_indexed_mut() {
        *val += 100;
    }

    assert_eq!(arena[first], 101);
    assert_eq!(arena[second], 102);
    assert_eq!(arena[third], 103);
}

#[test]
fn iter_indexed_mut_exact_size() {
    let mut arena = Arena::new();
    arena.alloc(1);
    arena.alloc(2);

    let iter = arena.iter_indexed_mut();
    assert_eq!(iter.len(), 2);
}

#[test]
fn reserve_increases_capacity() {
    let mut arena: Arena<u64> = Arena::new();
    arena.reserve(500);
    assert!(arena.capacity() >= 500);
    assert!(arena.is_empty());
}

#[test]
fn extend_trait() {
    let mut arena = Arena::new();
    arena.alloc(1);
    arena.extend(vec![2, 3, 4]);
    assert_eq!(arena.len(), 4);

    let values: Vec<_> = arena.iter().copied().collect();
    assert_eq!(values, vec![1, 2, 3, 4]);
}

#[test]
fn from_iterator() {
    let arena: Arena<i32> = (0..5).collect();
    assert_eq!(arena.len(), 5);
    let indexed: Vec<_> = arena
        .iter_indexed()
        .map(|(idx, value)| (idx.slot(), *value))
        .collect();
    assert_eq!(indexed, vec![(0, 0), (1, 1), (2, 2), (3, 3), (4, 4)]);
}

#[test]
fn checkpoint_equality() {
    let mut arena = Arena::new();
    let cp1 = arena.checkpoint();
    let cp2 = arena.checkpoint();
    assert_eq!(cp1, cp2);

    arena.alloc(1);
    let cp3 = arena.checkpoint();
    assert_ne!(cp1, cp3);
}

#[test]
fn checkpoint_ordering() {
    let mut arena = Arena::new();
    let cp1 = arena.checkpoint();
    arena.alloc(1);
    let cp2 = arena.checkpoint();
    arena.alloc(2);
    let cp3 = arena.checkpoint();

    assert!(cp1 < cp2);
    assert!(cp2 < cp3);
}

#[test]
fn drop_arena_runs_drop() {
    let drop_count = Rc::new(Cell::new(0u32));

    {
        let mut arena = Arena::new();
        arena.alloc(Tracked(Rc::clone(&drop_count)));
        arena.alloc(Tracked(Rc::clone(&drop_count)));
        arena.alloc(Tracked(Rc::clone(&drop_count)));
        assert_eq!(drop_count.get(), 0);
    } // arena dropped here

    assert_eq!(drop_count.get(), 3);
}

#[test]
fn checkpoint_keep() {
    let mut arena = Arena::new();
    let a = arena.alloc(1);
    let _cp = arena.checkpoint();

    // Allocate speculatively
    let b = arena.alloc(2);
    let c = arena.alloc(3);

    // Decide to KEEP — simply don't rollback
    assert_eq!(arena.len(), 3);
    assert_eq!(arena[a], 1);
    assert_eq!(arena[b], 2);
    assert_eq!(arena[c], 3);
}

#[test]
fn into_iter_consuming() {
    let mut arena = Arena::new();
    arena.alloc(String::from("a"));
    arena.alloc(String::from("b"));
    arena.alloc(String::from("c"));

    let collected: Vec<String> = arena.into_iter().collect();
    assert_eq!(collected, vec!["a", "b", "c"]);
}

#[test]
fn equal_slots_from_different_arenas_do_not_alias() {
    let mut left = Arena::new();
    let mut right = Arena::new();
    let left_idx = left.alloc(10);
    let right_idx = right.alloc(20);

    assert_eq!(left_idx.slot(), right_idx.slot());
    assert_ne!(left_idx, right_idx);
    assert_eq!(left.try_get(right_idx), None);
    assert_eq!(right.try_get(left_idx), None);
}

/// `Idx` carries a segment stamp and a slot, with no separate arena-owner
/// field (see `crate::segments`). This is the "foreign `Idx` whose slot is
/// in range" half of the argument for why that is still sound: `right` has
/// an item at the same slot `left_idx` names, so the length check alone
/// cannot reject it, and rejection depends entirely on `left_idx`'s stamp
/// never matching any stamp `right` has ever drawn — which holds because
/// every stamp is drawn from one process-wide sequence, not a per-arena one,
/// so two independently created arenas can never coin the same stamp value.
#[test]
fn is_valid_rejects_foreign_idx_with_slot_in_range() {
    let mut left = Arena::new();
    let mut right = Arena::new();
    for value in 0..8 {
        left.alloc(value);
        right.alloc(value + 100);
    }
    let left_idx = left.alloc(999);
    let _right_padding = right.alloc(999);

    assert_eq!(left_idx.slot(), 8);
    assert!(
        right.len() > left_idx.slot(),
        "the slot is in range for `right`"
    );
    assert!(!right.is_valid(left_idx));
    assert_eq!(right.try_get(left_idx), None);
}

/// The other half of the "no owner field" argument. An arena that has never
/// issued a capability has no identity at all yet, so `is_valid` cannot even
/// compare a stamp — but it also never needs to, because such an arena has
/// zero items, and the length check `idx.slot() < self.items.len()` rejects
/// every `Idx` before a stamp comparison would even be reached. This test
/// also confirms `is_valid` itself never forces identity assignment (unlike
/// `checkpoint`/`alloc`): a brand new, never-touched arena stays exactly
/// that way after `is_valid` calls that return `false`.
#[test]
fn is_valid_rejects_any_idx_on_an_arena_that_never_issued_a_capability() {
    let mut other = Arena::new();
    let foreign_idx = other.alloc(1);

    let never_touched: Arena<i32> = Arena::new();
    assert!(!never_touched.is_valid(foreign_idx));
    assert_eq!(never_touched.try_get(foreign_idx), None);
    // Confirm the "never touched" premise itself, i.e. that this really
    // exercises the never-assigned-identity path and not just an ordinary
    // stale/foreign rejection on an otherwise-used arena.
    assert!(never_touched.is_empty());
}

#[test]
fn reset_and_reallocation_do_not_resurrect_an_old_index() {
    let mut arena = Arena::new();
    let stale = arena.alloc(String::from("old"));
    arena.reset();
    let current = arena.alloc(String::from("new"));

    assert_eq!(stale.slot(), current.slot());
    assert_ne!(stale, current);
    assert_eq!(arena.try_get(stale), None);
    assert_eq!(arena.try_get(current).map(String::as_str), Some("new"));
}

#[test]
fn foreign_checkpoint_is_rejected_without_mutation() {
    let left: Arena<i32> = Arena::new();
    let foreign = left.checkpoint();
    let mut right = Arena::new();
    let live = right.alloc(7);

    assert_eq!(
        right.try_rollback(foreign),
        Err(CheckpointError::ForeignArena)
    );
    assert_eq!(right[live], 7);
}

#[test]
fn equal_length_checkpoint_from_a_discarded_branch_is_rejected() {
    let mut arena = Arena::new();
    let _root = arena.alloc(0);
    let branch_point = arena.checkpoint();
    let stale = arena.alloc(1);
    let stale_prefix = arena.checkpoint();

    arena.rollback(branch_point);
    let replacement = arena.alloc(2);
    assert_eq!(stale.slot(), replacement.slot());
    assert_eq!(arena.len(), stale_prefix.len());
    assert_eq!(arena.try_get(stale), None);
    assert_eq!(
        arena.try_rollback(stale_prefix),
        Err(CheckpointError::DivergedPrefix { checkpoint_len: 2 })
    );
    assert_eq!(arena[replacement], 2);
}

#[test]
fn validate_checkpoint_accepts_a_valid_checkpoint_without_mutating_the_arena() {
    let mut arena = Arena::new();
    arena.alloc(1);
    arena.alloc(2);
    let checkpoint = arena.checkpoint();
    arena.alloc(3);

    assert_eq!(arena.validate_checkpoint(checkpoint), Ok(()));
    assert_eq!(arena.len(), 3, "validation must not mutate the arena");
}

#[test]
fn validate_checkpoint_rejects_a_foreign_checkpoint() {
    let left: Arena<i32> = Arena::new();
    let foreign = left.checkpoint();
    let right = Arena::<i32>::new();

    assert_eq!(
        right.validate_checkpoint(foreign),
        Err(CheckpointError::ForeignArena)
    );
}

#[test]
fn validate_checkpoint_rejects_a_checkpoint_beyond_current_length() {
    let mut arena = Arena::new();
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
    let mut arena = Arena::new();
    let _root = arena.alloc(0);
    let branch_point = arena.checkpoint();
    let _stale = arena.alloc(1);
    let stale_prefix = arena.checkpoint();
    arena.rollback(branch_point);
    let _replacement = arena.alloc(2);

    assert_eq!(
        arena.validate_checkpoint(stale_prefix),
        Err(CheckpointError::DivergedPrefix { checkpoint_len: 2 })
    );
}

#[test]
fn validate_checkpoint_accepts_an_empty_arena_checkpoint_after_reset() {
    let mut arena = Arena::new();
    arena.alloc(1);
    arena.alloc(2);
    arena.reset();
    let empty_checkpoint = arena.checkpoint();

    assert_eq!(empty_checkpoint.len(), 0);
    assert!(empty_checkpoint.tail().is_none());
    assert_eq!(arena.validate_checkpoint(empty_checkpoint), Ok(()));
}

#[test]
fn rollback_keeps_metadata_aligned_when_a_destructor_panics() {
    struct PanicOnce(Rc<Cell<bool>>);

    impl Drop for PanicOnce {
        fn drop(&mut self) {
            assert!(self.0.replace(true), "intentional destructor panic");
        }
    }

    let tripped = Rc::new(Cell::new(false));
    let mut arena = Arena::new();
    let _kept = arena.alloc(PanicOnce(Rc::clone(&tripped)));
    let checkpoint = arena.checkpoint();
    let first_suffix = arena.alloc(PanicOnce(Rc::clone(&tripped)));
    let panicking_suffix = arena.alloc(PanicOnce(Rc::clone(&tripped)));

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        arena.rollback(checkpoint);
    }));
    assert!(result.is_err());
    assert_eq!(arena.len(), 2);
    assert!(!arena.is_valid(panicking_suffix));
    // The segment boundary commits before the drop loop runs, so
    // `first_suffix` (slot >= the checkpoint's target length) is already
    // rejected here even though its value has not been physically popped
    // yet — see `crate::segments`'s module documentation.
    assert!(!arena.is_valid(first_suffix));

    arena.rollback(checkpoint);
    assert_eq!(arena.len(), 1);
    assert!(!arena.is_valid(first_suffix));
}

/// Regression test for the ABA hole a commit-after-the-loop ordering would
/// allow: without retrying an interrupted rollback to completion, a caller
/// could `alloc` directly, reuse a not-yet-popped slot under the *old*
/// generation, and have a stale pre-rollback `Idx` for that slot validate
/// again against the new value. Committing the new segment boundary before
/// the drop loop (rather than after it) closes this: every slot at or above
/// the checkpoint's target length is rejected from the moment `rollback` is
/// entered, whether or not its value has been physically popped, and
/// whether or not the caller ever retries.
#[test]
fn rollback_without_retry_after_a_panic_on_the_second_dropped_slot_cannot_be_refilled_under_the_stale_generation()
 {
    struct PanicOnSecondDrop(Rc<Cell<u32>>);

    impl Drop for PanicOnSecondDrop {
        fn drop(&mut self) {
            let count = self.0.get() + 1;
            self.0.set(count);
            assert_ne!(count, 2, "intentional destructor panic on the second drop");
        }
    }

    let drop_count = Rc::new(Cell::new(0));
    let mut arena: Arena<PanicOnSecondDrop> = Arena::new();
    let _first = arena.alloc(PanicOnSecondDrop(Rc::clone(&drop_count))); // slot 0, survives
    let checkpoint = arena.checkpoint(); // target_len = 1
    let stale_a = arena.alloc(PanicOnSecondDrop(Rc::clone(&drop_count))); // slot 1, never popped
    let stale_b = arena.alloc(PanicOnSecondDrop(Rc::clone(&drop_count))); // slot 2, popped 2nd (panics)
    let stale_c = arena.alloc(PanicOnSecondDrop(Rc::clone(&drop_count))); // slot 3, popped 1st

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        arena.rollback(checkpoint);
    }));
    assert!(
        result.is_err(),
        "the second dropped value's destructor panics"
    );
    assert_eq!(
        drop_count.get(),
        2,
        "stale_c then stale_b are dropped before the panic surfaces"
    );
    assert_eq!(
        arena.len(),
        2,
        "only stale_c and stale_b were popped before the panic; stale_a is untouched"
    );

    // Without retrying the rollback, allocate twice. These reuse the exact
    // slots stale_b/stale_c used to name (2 and 3) — the historical bug —
    // and stale_a's slot (1) is still sitting there completely untouched.
    let refill_1 = arena.alloc(PanicOnSecondDrop(Rc::clone(&drop_count)));
    let refill_2 = arena.alloc(PanicOnSecondDrop(Rc::clone(&drop_count)));
    assert_eq!(refill_1.slot(), 2);
    assert_eq!(refill_2.slot(), 3);

    // Every pre-rollback `Idx` at or above the target length is rejected —
    // including `stale_a`, whose slot was never physically popped, and
    // `stale_b`/`stale_c`, whose slots were just refilled under a new
    // generation neither of them carries.
    assert!(!arena.is_valid(stale_a));
    assert!(!arena.is_valid(stale_b));
    assert!(!arena.is_valid(stale_c));
    // A fresh `Idx` validates normally.
    assert!(arena.is_valid(refill_1));
    assert!(arena.is_valid(refill_2));

    // The retry path still works: rolling back to the same checkpoint again
    // converges to the target length — dropping the orphaned `stale_a`
    // value and both refills — and a fresh allocation afterward validates.
    arena.rollback(checkpoint);
    assert_eq!(arena.len(), 1);
    assert!(!arena.is_valid(refill_1));
    assert!(!arena.is_valid(refill_2));
    assert_eq!(
        drop_count.get(),
        5,
        "stale_a and both refills are dropped by the retried rollback"
    );

    let after_retry = arena.alloc(PanicOnSecondDrop(Rc::clone(&drop_count)));
    assert!(arena.is_valid(after_retry));
    assert_eq!(after_retry.slot(), 1);
}

#[test]
fn reset_with_a_panicking_drop_keeps_alignment_and_is_retryable() {
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
    let mut arena = Arena::new();
    let kept = arena.alloc(Tracked {
        drops: Rc::clone(&drops),
        panic_on_drop: false,
    });
    let panicking = arena.alloc(Tracked {
        drops: Rc::clone(&drops),
        panic_on_drop: true,
    });
    let last = arena.alloc(Tracked {
        drops: Rc::clone(&drops),
        panic_on_drop: false,
    });

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| arena.reset()));
    assert!(result.is_err());
    assert_eq!(
        drops.get(),
        2,
        "last and panicking are dropped from the end before the panic surfaces"
    );
    assert_eq!(arena.len(), 1);
    // `reset` always targets length 0, so its segment boundary commits
    // before the drop loop covers *every* existing slot, including `kept`
    // (not yet physically popped when `panicking`'s destructor fired).
    // `kept` is therefore already rejected here, not just once its slot is
    // later reused — see `crate::segments`'s module documentation.
    assert!(!arena.is_valid(kept));
    assert!(!arena.is_valid(panicking));
    assert!(!arena.is_valid(last));
    assert_eq!(arena.iter_indexed().count(), 1);

    arena.reset();
    assert_eq!(drops.get(), 3);
    assert!(arena.is_empty());

    let replacement = arena.alloc(Tracked {
        drops: Rc::clone(&drops),
        panic_on_drop: false,
    });
    assert!(arena.is_valid(replacement));
    assert!(!arena.is_valid(kept));
    assert_eq!(replacement.slot(), kept.slot());
}

#[test]
fn alloc_block_with_a_panicking_iterator_retains_prefix_and_capacity() {
    struct Tracked {
        drops: Rc<Cell<u32>>,
    }

    impl Drop for Tracked {
        fn drop(&mut self) {
            self.drops.set(self.drops.get() + 1);
        }
    }

    let drops = Rc::new(Cell::new(0));
    let mut arena = Arena::new();
    let kept = arena.alloc(Tracked {
        drops: Rc::clone(&drops),
    });
    let capacity_before = arena.capacity();
    let checkpoint = arena.checkpoint();

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        arena.alloc_block((0..5).map(|i| {
            assert!(i != 3, "the input iterator panics partway through");
            Tracked {
                drops: Rc::clone(&drops),
            }
        }))
    }));

    assert!(result.is_err());
    assert_eq!(arena.len(), 1);
    assert!(arena.is_valid(kept));
    assert_eq!(arena.capacity(), capacity_before);
    assert_eq!(
        drops.get(),
        3,
        "the three collected values drop with the local buffer"
    );
    assert_eq!(arena.try_rollback(checkpoint), Ok(()));
}

#[test]
fn drain_forgotten_leaks_without_a_double_drop() {
    struct Tracked {
        drops: Rc<Cell<u32>>,
    }

    impl Drop for Tracked {
        fn drop(&mut self) {
            self.drops.set(self.drops.get() + 1);
        }
    }

    let drops = Rc::new(Cell::new(0));
    let mut arena = Arena::new();
    let old = arena.alloc(Tracked {
        drops: Rc::clone(&drops),
    });
    arena.alloc(Tracked {
        drops: Rc::clone(&drops),
    });

    std::mem::forget(arena.drain());

    assert_eq!(arena.len(), 0);
    assert!(!arena.is_valid(old));
    let replacement = arena.alloc(Tracked {
        drops: Rc::clone(&drops),
    });
    assert!(arena.is_valid(replacement));
    assert_eq!(arena.iter_indexed().count(), 1);
    assert_eq!(
        drops.get(),
        0,
        "forgetting the drain iterator leaks its values, matching std::mem::forget \
         semantics, instead of dropping them twice"
    );
}

#[test]
fn drain_with_a_panicking_drop_keeps_alignment_and_is_retryable() {
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
    let mut arena = Arena::new();
    let panicking = arena.alloc(Tracked {
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
    assert!(!arena.is_valid(panicking));
    assert_eq!(
        drops.get(),
        2,
        "both drained values are dropped exactly once"
    );

    let replacement = arena.alloc(Tracked {
        drops: Rc::clone(&drops),
        panic_on_drop: false,
    });
    assert!(arena.is_valid(replacement));
    assert_eq!(arena.iter_indexed().count(), 1);
}

#[test]
fn checkpoint_taken_before_drain_is_rejected_then_reported_as_diverged() {
    let mut arena = Arena::new();
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
fn alloc_block_reentrant_iterator_through_a_refcell_hits_a_borrow_error() {
    let cell = std::cell::RefCell::new(Arena::new());
    cell.borrow_mut().alloc(0);

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut guard = cell.borrow_mut();
        let source = (0..3).inspect(|&i| {
            if i == 1 {
                // Would mutate the same arena through the iterator if the
                // dynamic borrow allowed a second exclusive access here.
                let _ = cell.borrow_mut();
            }
        });
        let _ = guard.alloc_block(source);
    }));

    assert!(
        result.is_err(),
        "the reentrant borrow_mut panics with BorrowMutError"
    );
    assert_eq!(cell.borrow().len(), 1);
}

#[test]
fn arena_creation_inside_a_tls_destructor_at_thread_exit_works() {
    use std::sync::atomic::{AtomicBool, Ordering};

    static OK: AtomicBool = AtomicBool::new(false);

    struct OnExit;

    impl Drop for OnExit {
        fn drop(&mut self) {
            let mut arena = Arena::new();
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
