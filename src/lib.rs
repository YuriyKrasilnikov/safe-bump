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

    /// Returns a mutable iterator over all allocated items.
    pub fn iter_mut(&mut self) -> std::slice::IterMut<'_, T> {
        self.items.iter_mut()
    }

    /// Allocates multiple values from an iterator, returning the index
    /// of the first allocated item.
    ///
    /// Returns `None` if the iterator is empty.
    ///
    /// O(n) where n = items yielded by the iterator.
    pub fn alloc_extend(&mut self, iter: impl IntoIterator<Item = T>) -> Option<Idx<T>> {
        let start = self.items.len();
        self.items.extend(iter);
        if self.items.len() > start {
            Some(Idx {
                index: start,
                _marker: PhantomData,
            })
        } else {
            None
        }
    }

    /// Returns `true` if `idx` points to a valid item in this arena.
    ///
    /// An index becomes invalid after [`rollback`](Arena::rollback) or
    /// [`reset`](Arena::reset) removes the item it pointed to.
    #[must_use]
    pub const fn is_valid(&self, idx: Idx<T>) -> bool {
        idx.index < self.items.len()
    }

    /// Returns a reference to the value at `idx`, or `None` if the
    /// index is out of bounds.
    #[must_use]
    pub fn try_get(&self, idx: Idx<T>) -> Option<&T> {
        self.items.get(idx.index)
    }

    /// Returns a mutable reference to the value at `idx`, or `None`
    /// if the index is out of bounds.
    #[must_use]
    pub fn try_get_mut(&mut self, idx: Idx<T>) -> Option<&mut T> {
        self.items.get_mut(idx.index)
    }

    /// Removes all items, returning an iterator that yields them
    /// in allocation order.
    ///
    /// The arena is empty after the iterator is consumed or dropped.
    /// Capacity is retained.
    pub fn drain(&mut self) -> std::vec::Drain<'_, T> {
        self.items.drain(..)
    }

    /// Returns an iterator yielding `(Idx<T>, &T)` pairs in allocation order.
    #[must_use]
    pub fn iter_indexed(&self) -> IterIndexed<'_, T> {
        IterIndexed {
            inner: self.items.iter().enumerate(),
        }
    }

    /// Returns a mutable iterator yielding `(Idx<T>, &mut T)` pairs in
    /// allocation order.
    pub fn iter_indexed_mut(&mut self) -> IterIndexedMut<'_, T> {
        IterIndexedMut {
            inner: self.items.iter_mut().enumerate(),
        }
    }

    /// Reserves capacity for at least `additional` more items.
    pub fn reserve(&mut self, additional: usize) {
        self.items.reserve(additional);
    }

    /// Shrinks the backing storage to fit the current number of items.
    pub fn shrink_to_fit(&mut self) {
        self.items.shrink_to_fit();
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

impl<'a, T> IntoIterator for &'a mut Arena<T> {
    type Item = &'a mut T;
    type IntoIter = std::slice::IterMut<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter_mut()
    }
}

impl<T> Extend<T> for Arena<T> {
    fn extend<I: IntoIterator<Item = T>>(&mut self, iter: I) {
        self.items.extend(iter);
    }
}

impl<T> std::iter::FromIterator<T> for Arena<T> {
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
        Self {
            items: iter.into_iter().collect(),
        }
    }
}

impl<T> IntoIterator for Arena<T> {
    type Item = T;
    type IntoIter = std::vec::IntoIter<T>;

    fn into_iter(self) -> Self::IntoIter {
        self.items.into_iter()
    }
}

// ─── IterIndexed ─────────────────────────────────────────────────────────────

/// Iterator yielding `(Idx<T>, &T)` pairs in allocation order.
///
/// Created by [`Arena::iter_indexed`].
pub struct IterIndexed<'a, T> {
    inner: std::iter::Enumerate<std::slice::Iter<'a, T>>,
}

impl<'a, T> Iterator for IterIndexed<'a, T> {
    type Item = (Idx<T>, &'a T);

    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next().map(|(i, v)| {
            (
                Idx {
                    index: i,
                    _marker: PhantomData,
                },
                v,
            )
        })
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

impl<T> ExactSizeIterator for IterIndexed<'_, T> {}

// ─── IterIndexedMut ─────────────────────────────────────────────────────────

/// Mutable iterator yielding `(Idx<T>, &mut T)` pairs in allocation order.
///
/// Created by [`Arena::iter_indexed_mut`].
pub struct IterIndexedMut<'a, T> {
    inner: std::iter::Enumerate<std::slice::IterMut<'a, T>>,
}

impl<'a, T> Iterator for IterIndexedMut<'a, T> {
    type Item = (Idx<T>, &'a mut T);

    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next().map(|(i, v)| {
            (
                Idx {
                    index: i,
                    _marker: PhantomData,
                },
                v,
            )
        })
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

impl<T> ExactSizeIterator for IterIndexedMut<'_, T> {}

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

impl<T> PartialEq for Checkpoint<T> {
    fn eq(&self, other: &Self) -> bool {
        self.len == other.len
    }
}

impl<T> Eq for Checkpoint<T> {}

impl<T> std::hash::Hash for Checkpoint<T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.len.hash(state);
    }
}

impl<T> std::fmt::Debug for Checkpoint<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Checkpoint({})", self.len)
    }
}

impl<T> PartialOrd for Checkpoint<T> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl<T> Ord for Checkpoint<T> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.len.cmp(&other.len)
    }
}

#[cfg(test)]
mod tests;
