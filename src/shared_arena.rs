use std::sync::atomic::{AtomicUsize, Ordering};

use crate::chunked_storage::ChunkedStorage;
use crate::{Checkpoint, Idx};

/// Thread-safe typed arena allocator.
///
/// Concurrent allocation via `&self`. Wait-free reads returning `&T`
/// directly (no guards or locks). Same [`Idx<T>`] handles and
/// [`Checkpoint<T>`] semantics as [`Arena`](crate::Arena).
///
/// `SharedArena<T>` is `Send + Sync` when `T: Send + Sync`.
///
/// For single-thread usage with zero overhead, see [`Arena`](crate::Arena).
pub struct SharedArena<T> {
    storage: ChunkedStorage<T>,
    /// Next slot to be reserved by `alloc`.
    reserved: AtomicUsize,
    /// Last index visible to readers (all slots `< published` are readable).
    published: AtomicUsize,
}

impl<T> SharedArena<T> {
    /// Creates an empty shared arena.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            storage: ChunkedStorage::new(),
            reserved: AtomicUsize::new(0),
            published: AtomicUsize::new(0),
        }
    }

    /// Allocates a value, returning its stable index.
    ///
    /// Can be called concurrently from multiple threads (`&self`).
    /// O(1).
    ///
    /// # Panics
    ///
    /// Panics if internal slot reservation fails (should not happen in
    /// normal usage).
    pub fn alloc(&self, value: T) -> Idx<T> {
        let slot = self.reserved.fetch_add(1, Ordering::Relaxed);
        let ok = self.storage.set(slot, value);
        assert!(ok, "slot {slot} already occupied");
        self.advance_published(slot);
        Idx::from_raw(slot)
    }

    /// Cooperatively advances `published` past `slot`.
    ///
    /// `published` always equals the length of the longest contiguous
    /// prefix of written slots: if slots 0..N are all written,
    /// `published = N`. This guarantees `get(i)` for any `i < published`
    /// will find a value — no gaps.
    ///
    /// Each writer helps advance `published` through all preceding ready
    /// slots, not just its own. If slots 5, 6, 7 complete out of order,
    /// the thread finishing slot 5 advances published 5→6→7→8 in one pass.
    ///
    /// # Spin behavior
    ///
    /// A thread spins only while its predecessor slot is not yet written.
    /// Between `reserved.fetch_add` and `storage.set` there is no user
    /// code — just `split_index` (pure math), `get_or_init` (chunk
    /// allocation), and `OnceLock::set` (memcpy). None of these can
    /// panic in normal operation, so the spin resolves in nanoseconds.
    /// A permanent stall would require the predecessor thread to abort
    /// (e.g. OOM or `process::abort`), which terminates the entire
    /// process anyway.
    fn advance_published(&self, slot: usize) {
        loop {
            let p = self.published.load(Ordering::Acquire);
            if p > slot {
                break; // Already published past our slot
            }
            // Check if slot at position p is written (by us or another thread)
            if !self.storage.is_set(p) {
                // Slot p not yet written by its owner. Spin briefly.
                std::hint::spin_loop();
                continue;
            }
            // Slot p is written. Try to advance published from p to p+1.
            // If CAS fails, another thread advanced — retry from new p.
            let _ = self.published.compare_exchange_weak(
                p,
                p + 1,
                Ordering::Release,
                Ordering::Relaxed,
            );
        }
    }

    /// Returns a reference to the value at `idx`.
    ///
    /// Wait-free. Returns `&T` directly (no guard or lock).
    ///
    /// # Panics
    ///
    /// Panics if `idx` is out of bounds (stale after rollback/reset).
    #[must_use]
    pub fn get(&self, idx: Idx<T>) -> &T {
        let i = idx.into_raw();
        assert!(
            i < self.published.load(Ordering::Acquire),
            "index out of bounds: index is {i} but published length is {}",
            self.published.load(Ordering::Acquire),
        );
        self.storage.get(i)
    }

    /// Returns the number of published (visible) items.
    #[must_use]
    pub fn len(&self) -> usize {
        self.published.load(Ordering::Acquire)
    }

    /// Returns `true` if the arena contains no items.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Saves the current allocation state.
    ///
    /// Use with [`rollback`](SharedArena::rollback) to discard allocations
    /// made after this point.
    #[must_use]
    pub fn checkpoint(&self) -> Checkpoint<T> {
        Checkpoint::from_len(self.published.load(Ordering::Acquire))
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
        let current = *self.published.get_mut();
        assert!(
            cp.len() <= current,
            "checkpoint {} beyond current length {}",
            cp.len(),
            current,
        );
        // Drop values in reverse order (mirrors Vec::truncate behavior)
        for slot in (cp.len()..current).rev() {
            self.storage.take(slot);
        }
        *self.published.get_mut() = cp.len();
        *self.reserved.get_mut() = cp.len();
    }

    /// Removes all items, running their destructors.
    ///
    /// Retains allocated storage for reuse.
    pub fn reset(&mut self) {
        let current = *self.published.get_mut();
        for slot in (0..current).rev() {
            self.storage.take(slot);
        }
        *self.published.get_mut() = 0;
        *self.reserved.get_mut() = 0;
    }

    /// Returns `true` if `idx` points to a valid item in this arena.
    #[must_use]
    pub fn is_valid(&self, idx: Idx<T>) -> bool {
        idx.into_raw() < self.published.load(Ordering::Acquire)
    }

    /// Returns a reference to the value at `idx`, or `None` if the
    /// index is out of bounds.
    #[must_use]
    pub fn try_get(&self, idx: Idx<T>) -> Option<&T> {
        let i = idx.into_raw();
        if i < self.published.load(Ordering::Acquire) {
            Some(self.storage.get(i))
        } else {
            None
        }
    }

    /// Allocates multiple values from an iterator, returning the index
    /// of the first allocated item.
    ///
    /// Returns `None` if the iterator is empty.
    ///
    /// O(n) where n = items yielded by the iterator.
    pub fn alloc_extend(&self, iter: impl IntoIterator<Item = T>) -> Option<Idx<T>> {
        let mut first = None;
        for value in iter {
            let idx = self.alloc(value);
            if first.is_none() {
                first = Some(idx);
            }
        }
        first
    }

    /// Removes all items, returning an iterator that yields them
    /// in allocation order.
    ///
    /// The arena is empty after the iterator is consumed or dropped.
    pub fn drain(&mut self) -> std::vec::IntoIter<T> {
        let current = *self.published.get_mut();
        let mut items = Vec::with_capacity(current);
        for slot in 0..current {
            if let Some(value) = self.storage.take(slot) {
                items.push(value);
            }
        }
        *self.published.get_mut() = 0;
        *self.reserved.get_mut() = 0;
        items.into_iter()
    }
}

impl<T> Default for SharedArena<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> std::ops::Index<Idx<T>> for SharedArena<T> {
    type Output = T;

    fn index(&self, idx: Idx<T>) -> &T {
        self.get(idx)
    }
}

impl<T> IntoIterator for SharedArena<T> {
    type Item = T;
    type IntoIter = std::vec::IntoIter<T>;

    fn into_iter(mut self) -> Self::IntoIter {
        self.drain()
    }
}
