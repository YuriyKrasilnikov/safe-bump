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
#[should_panic(expected = "checkpoint 5 beyond current length 2")]
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
#[should_panic(expected = "index out of bounds")]
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
    let a = Idx::<i32>::from_raw(5);
    let b = Idx::<i32>::from_raw(5);
    let c = Idx::<i32>::from_raw(3);

    assert_eq!(a, b);
    assert_ne!(a, c);
}

#[test]
fn idx_ordering() {
    let a = Idx::<i32>::from_raw(1);
    let b = Idx::<i32>::from_raw(5);

    assert!(a < b);
}

#[test]
fn idx_raw_roundtrip() {
    let idx = Idx::<String>::from_raw(42);
    assert_eq!(idx.into_raw(), 42);
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
fn alloc_extend_returns_first_idx() {
    let mut arena = Arena::new();
    arena.alloc(0);

    let first = arena.alloc_extend(vec![10, 20, 30]);
    assert_eq!(first, Some(Idx::from_raw(1)));
    assert_eq!(arena.len(), 4);
    assert_eq!(arena[Idx::from_raw(1)], 10);
    assert_eq!(arena[Idx::from_raw(2)], 20);
    assert_eq!(arena[Idx::from_raw(3)], 30);
}

#[test]
fn alloc_extend_empty_returns_none() {
    let mut arena: Arena<i32> = Arena::new();
    let result = arena.alloc_extend(std::iter::empty());
    assert_eq!(result, None);
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
    arena.alloc(1);
    arena.alloc(2);
    arena.alloc(3);

    for (_, val) in arena.iter_indexed_mut() {
        *val += 100;
    }

    assert_eq!(arena[Idx::from_raw(0)], 101);
    assert_eq!(arena[Idx::from_raw(1)], 102);
    assert_eq!(arena[Idx::from_raw(2)], 103);
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
    assert_eq!(arena[Idx::from_raw(0)], 0);
    assert_eq!(arena[Idx::from_raw(4)], 4);
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
