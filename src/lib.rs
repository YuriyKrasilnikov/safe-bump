//! Safe bump-pointer arena allocator.
//!
//! `safe-bump` provides a typed arena allocator built entirely with safe Rust
//! (zero `unsafe` blocks). Values are allocated by appending to an internal
//! buffer and accessed via stable [`Idx<T>`] indices.
//!
//! # Key properties
//!
//! - **Zero `unsafe`**: enforced by `#![forbid(unsafe_code)]`
//! - **Auto [`Drop`]**: destructors run on reset, rollback, and arena drop
//! - **Checkpoint/rollback**: save state and discard speculative allocations
//! - **O(1) amortized allocation**: backed by [`Vec::push`]
//! - **O(1) index access**: direct indexing into backing storage
//!
//! # Example
//!
//! ```
//! use safe_bump::{Arena, Idx};
//!
//! let mut arena: Arena<String> = Arena::new();
//! let a: Idx<String> = arena.alloc(String::from("hello"));
//! let b: Idx<String> = arena.alloc(String::from("world"));
//!
//! assert_eq!(arena[a], "hello");
//! assert_eq!(arena[b], "world");
//! assert_eq!(arena.len(), 2);
//!
//! // Checkpoint and rollback
//! let cp = arena.checkpoint();
//! let _tmp = arena.alloc(String::from("temporary"));
//! assert_eq!(arena.len(), 3);
//!
//! arena.rollback(cp); // "temporary" is dropped
//! assert_eq!(arena.len(), 2);
//! ```
//!
//! # References
//!
//! - Hanson, 1990 — "Fast Allocation and Deallocation of Memory
//!   Based on Object Lifetimes"

#![forbid(unsafe_code)]
#![deny(missing_docs)]

use std::marker::PhantomData;

/// Typed arena allocator.
///
/// Stores values of type `T` in a contiguous buffer, returning stable
/// [`Idx<T>`] handles for O(1) access. Values are dropped when the arena
/// is dropped, reset, or rolled back past their allocation point.
///
/// # Differences from other arena crates
///
/// | Feature | `safe-bump` | `bumpalo` | `typed-arena` | `bump-scope` |
/// |---------|------------|-----------|---------------|-------------|
/// | `unsafe` code | none | yes | yes | yes |
/// | Auto `Drop` | yes | no | yes | yes |
/// | Checkpoint/rollback | yes | no | no | scopes (LIFO only) |
/// | Keep OR discard | yes | no | no | discard only |
/// | Access pattern | `Idx<T>` | `&T` | `&T` | `BumpBox<T>` |
pub struct Arena<T> {
    items: Vec<T>,
}

/// Stable index into an [`Arena<T>`].
///
/// Obtained from [`Arena::alloc`]. Implements [`Copy`], so it can be
/// freely duplicated and stored in data structures.
///
/// Valid as long as the arena has not been reset or rolled back past
/// this index.
///
/// # Panics
///
/// Indexing with a stale `Idx` (after rollback/reset) panics with
/// an out-of-bounds error.
pub struct Idx<T> {
    index: usize,
    _marker: PhantomData<T>,
}

/// Saved allocation state for [`Arena::rollback`].
///
/// Created by [`Arena::checkpoint`]. Rolling back to a checkpoint
/// drops all values allocated after it and retains everything before.
pub struct Checkpoint<T> {
    len: usize,
    _marker: PhantomData<T>,
}

// ─── Arena ───────────────────────────────────────────────────────────────────

impl<T> Arena<T> {
    /// Creates an empty arena.
    #[must_use]
    pub const fn new() -> Self {
        Self { items: Vec::new() }
    }

    /// Creates an arena with pre-allocated capacity for `capacity` items.
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            items: Vec::with_capacity(capacity),
        }
    }

    /// Allocates a value in the arena, returning its stable index.
    ///
    /// O(1) amortized (backed by [`Vec::push`]).
    pub fn alloc(&mut self, value: T) -> Idx<T> {
        let index = self.items.len();
        self.items.push(value);
        Idx {
            index,
            _marker: PhantomData,
        }
    }

    /// Returns a reference to the value at `idx`.
    ///
    /// # Panics
    ///
    /// Panics if `idx` is out of bounds (stale after rollback/reset).
    #[must_use]
    pub fn get(&self, idx: Idx<T>) -> &T {
        &self.items[idx.index]
    }

    /// Returns a mutable reference to the value at `idx`.
    ///
    /// # Panics
    ///
    /// Panics if `idx` is out of bounds (stale after rollback/reset).
    #[must_use]
    pub fn get_mut(&mut self, idx: Idx<T>) -> &mut T {
        &mut self.items[idx.index]
    }

    /// Returns the number of allocated items.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.items.len()
    }

    /// Returns `true` if the arena contains no items.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Returns the current capacity in items.
    #[must_use]
    pub const fn capacity(&self) -> usize {
        self.items.capacity()
    }

    /// Saves the current allocation state.
    ///
    /// Use with [`rollback`](Arena::rollback) to discard allocations
    /// made after this point.
    #[must_use]
    pub const fn checkpoint(&self) -> Checkpoint<T> {
        Checkpoint {
            len: self.items.len(),
            _marker: PhantomData,
        }
    }

    /// Rolls back to a previous checkpoint, dropping all values
    /// allocated after it.
    ///
    /// O(k) where k = number of items dropped (destructors run).
    ///
    /// # Panics
    ///
    /// Panics if `cp` points beyond the current length.
    pub fn rollback(&mut self, cp: Checkpoint<T>) {
        assert!(
            cp.len <= self.items.len(),
            "checkpoint {} beyond current length {}",
            cp.len,
            self.items.len(),
        );
        self.items.truncate(cp.len);
    }

    /// Removes all items, running their destructors.
    ///
    /// Retains allocated memory for reuse.
    pub fn reset(&mut self) {
        self.items.clear();
    }

    /// Returns an iterator over all allocated items.
    pub fn iter(&self) -> std::slice::Iter<'_, T> {
        self.items.iter()
    }
}

impl<T> Default for Arena<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> std::ops::Index<Idx<T>> for Arena<T> {
    type Output = T;

    fn index(&self, idx: Idx<T>) -> &T {
        self.get(idx)
    }
}

impl<T> std::ops::IndexMut<Idx<T>> for Arena<T> {
    fn index_mut(&mut self, idx: Idx<T>) -> &mut T {
        self.get_mut(idx)
    }
}

impl<'a, T> IntoIterator for &'a Arena<T> {
    type Item = &'a T;
    type IntoIter = std::slice::Iter<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

// ─── Idx ─────────────────────────────────────────────────────────────────────

impl<T> Idx<T> {
    /// Returns the raw index value.
    #[must_use]
    pub const fn into_raw(self) -> usize {
        self.index
    }

    /// Creates an index from a raw value.
    ///
    /// The caller must ensure the index is valid for the target arena.
    #[must_use]
    pub const fn from_raw(index: usize) -> Self {
        Self {
            index,
            _marker: PhantomData,
        }
    }
}

impl<T> Clone for Idx<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for Idx<T> {}

impl<T> PartialEq for Idx<T> {
    fn eq(&self, other: &Self) -> bool {
        self.index == other.index
    }
}

impl<T> Eq for Idx<T> {}

impl<T> std::hash::Hash for Idx<T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.index.hash(state);
    }
}

impl<T> std::fmt::Debug for Idx<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Idx({})", self.index)
    }
}

impl<T> PartialOrd for Idx<T> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl<T> Ord for Idx<T> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.index.cmp(&other.index)
    }
}

// ─── Checkpoint ──────────────────────────────────────────────────────────────

impl<T> Checkpoint<T> {
    /// Returns the saved length.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.len
    }

    /// Returns `true` if the checkpoint was taken at an empty state.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }
}

impl<T> Clone for Checkpoint<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for Checkpoint<T> {}

impl<T> std::fmt::Debug for Checkpoint<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Checkpoint({})", self.len)
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
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

        struct Tracked(Rc<Cell<u32>>);
        impl Drop for Tracked {
            fn drop(&mut self) {
                self.0.set(self.0.get() + 1);
            }
        }

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

        struct Tracked(Rc<Cell<u32>>);
        impl Drop for Tracked {
            fn drop(&mut self) {
                self.0.set(self.0.get() + 1);
            }
        }

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

        let bad_cp = Checkpoint::<i32> {
            len: 5,
            _marker: PhantomData,
        };
        arena.rollback(bad_cp);
    }

    #[test]
    #[should_panic]
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
}
