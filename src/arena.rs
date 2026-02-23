use crate::{Checkpoint, Idx, IterIndexed, IterIndexedMut};

/// Single-thread typed arena allocator.
///
/// Stores values of type `T` in a contiguous buffer, returning stable
/// [`Idx<T>`] handles for O(1) access. Values are dropped when the arena
/// is dropped, reset, or rolled back past their allocation point.
///
/// For thread-safe concurrent allocation, see [`SharedArena`](crate::SharedArena).
pub struct Arena<T> {
    items: Vec<T>,
}

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
        Idx::from_raw(index)
    }

    /// Returns a reference to the value at `idx`.
    ///
    /// # Panics
    ///
    /// Panics if `idx` is out of bounds (stale after rollback/reset).
    #[must_use]
    pub fn get(&self, idx: Idx<T>) -> &T {
        &self.items[idx.into_raw()]
    }

    /// Returns a mutable reference to the value at `idx`.
    ///
    /// # Panics
    ///
    /// Panics if `idx` is out of bounds (stale after rollback/reset).
    #[must_use]
    pub fn get_mut(&mut self, idx: Idx<T>) -> &mut T {
        &mut self.items[idx.into_raw()]
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
        Checkpoint::from_len(self.items.len())
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
            cp.len() <= self.items.len(),
            "checkpoint {} beyond current length {}",
            cp.len(),
            self.items.len(),
        );
        self.items.truncate(cp.len());
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
            Some(Idx::from_raw(start))
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
        idx.into_raw() < self.items.len()
    }

    /// Returns a reference to the value at `idx`, or `None` if the
    /// index is out of bounds.
    #[must_use]
    pub fn try_get(&self, idx: Idx<T>) -> Option<&T> {
        self.items.get(idx.into_raw())
    }

    /// Returns a mutable reference to the value at `idx`, or `None`
    /// if the index is out of bounds.
    #[must_use]
    pub fn try_get_mut(&mut self, idx: Idx<T>) -> Option<&mut T> {
        self.items.get_mut(idx.into_raw())
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
        IterIndexed::new(self.items.iter().enumerate())
    }

    /// Returns a mutable iterator yielding `(Idx<T>, &mut T)` pairs in
    /// allocation order.
    pub fn iter_indexed_mut(&mut self) -> IterIndexedMut<'_, T> {
        IterIndexedMut::new(self.items.iter_mut().enumerate())
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
