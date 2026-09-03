use std::sync::atomic::{AtomicUsize, Ordering};

use crate::chunked_storage::ChunkedStorage;
use crate::segments::SharedIdentity;
use crate::{Block, Checkpoint, CheckpointError, Idx};

/// Experimental concurrent typed arena.
///
/// Reads of already-published indices are wait-free and return `&T` directly.
/// Allocation reserves a range atomically and cooperatively advances a
/// contiguous publication prefix. It can wait for an earlier reserving thread;
/// therefore allocation is deliberately **not** described as wait-free.
///
/// Storage slots hold plain `T` values, with no per-slot stamp wrapper:
/// capability validation is backed by one lazily assigned identity (birth
/// stamp, current segment stamp, and a small archived-segment table), the
/// same mechanism [`Arena`](crate::Arena) uses — see `crate::segments` for
/// the full mechanism.
///
/// This type is available only with the `experimental-shared` feature. Its API
/// may change before the feature is stabilized.
pub struct SharedArena<T> {
    identity: SharedIdentity,
    storage: ChunkedStorage<T>,
    reserved: AtomicUsize,
    published: AtomicUsize,
}

impl<T> SharedArena<T> {
    /// Creates an empty concurrent arena. The identity is not assigned yet;
    /// it is created lazily by the first operation that hands out an `Idx`,
    /// `Block`, or `Checkpoint`.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            identity: SharedIdentity::new(),
            storage: ChunkedStorage::new(),
            reserved: AtomicUsize::new(0),
            published: AtomicUsize::new(0),
        }
    }

    /// Allocates one value through a shared reference.
    ///
    /// Publication may busy-wait for an earlier reserved slot to be filled.
    ///
    /// # Panics
    ///
    /// Panics if the process exhausts the stamp or slot space, or if an
    /// internal storage invariant reports an already-occupied reserved slot.
    pub fn alloc(&self, value: T) -> Idx<T> {
        let stamp = self.identity.current();
        let slot = self.reserve_range(1);
        let inserted = self.storage.set(slot, value);
        assert!(inserted, "reserved slot {slot} was already occupied");
        self.advance_published(slot);
        Idx::new(stamp, slot)
    }

    /// Allocates one contiguous batch through a single atomic reservation.
    ///
    /// Input collection happens before reservation. Concurrent allocations may
    /// occur before or after this block, but cannot interleave its slots.
    /// Publication may busy-wait for an earlier reserved range to be filled.
    ///
    /// # Panics
    ///
    /// The input iterator may panic while it is collected, before this call
    /// reserves or publishes anything: this method's own reservation and
    /// publication effects do not happen in that case. This is not a
    /// guarantee that the arena as a whole is unchanged. `SharedArena` takes
    /// `&self`, so the supplied iterator can itself hold another reference to
    /// this same arena (from this thread or another) and allocate through it
    /// — including via reentrant calls made from its own `Iterator::next` —
    /// before panicking; any such reentrant allocation is the iterator's own
    /// effect, is not rolled back, and stays published. Compare
    /// [`Arena::alloc_block`](crate::Arena::alloc_block), which takes
    /// `&mut self` and therefore statically excludes a reentrant caller-side
    /// mutation. The method also panics on stamp/slot exhaustion or
    /// violation of an internal reserved-slot invariant.
    pub fn alloc_block(&self, iter: impl IntoIterator<Item = T>) -> Block<T> {
        let values: Vec<T> = iter.into_iter().collect();
        let len = values.len();
        if len == 0 {
            return Block::empty();
        }

        let stamp = self.identity.current();
        let start = self.reserve_range(len);
        for (offset, value) in values.into_iter().enumerate() {
            let slot = start
                .checked_add(offset)
                .expect("a reserved allocation range has a representable end");
            let inserted = self.storage.set(slot, value);
            assert!(inserted, "reserved slot {slot} was already occupied");
        }
        let last = start
            .checked_add(len - 1)
            .expect("a reserved allocation range has a representable end");
        self.advance_published(last);
        Block::new(stamp, start, len)
    }

    /// Returns a reference to a published value.
    ///
    /// # Panics
    ///
    /// Panics for an unpublished, foreign, or stale capability.
    #[inline]
    #[must_use]
    pub fn get(&self, idx: Idx<T>) -> &T {
        self.try_get(idx)
            .unwrap_or_else(|| unpublished_index_panic(idx))
    }

    /// Returns the length of the contiguous published prefix.
    #[must_use]
    pub fn len(&self) -> usize {
        self.published.load(Ordering::Acquire)
    }

    /// Returns `true` when no values are published.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Captures the currently published historical prefix.
    #[must_use]
    pub fn checkpoint(&self) -> Checkpoint<T> {
        let len = self.len();
        let tail = len.checked_sub(1).map(|slot| self.identity.stamp_of(slot));
        Checkpoint::new(self.identity.birth(), len, tail)
    }

    /// Validates and rolls back to a historical prefix.
    ///
    /// Exclusive access statically excludes concurrent allocation during the
    /// rollback.
    ///
    /// # Errors
    ///
    /// Returns [`CheckpointError`] for a foreign checkpoint, a checkpoint
    /// beyond the current length, or a historical prefix that was replaced.
    ///
    /// # Panics
    ///
    /// A discarded value's destructor may panic. The new segment boundary is
    /// committed *before* any value is taken and dropped, so every slot
    /// `>= checkpoint.len()` is already rejected by
    /// [`is_valid`](Self::is_valid) from the moment this call begins —
    /// closing an ABA hole a commit-after-the-loop ordering would have (see
    /// [`Arena::try_rollback`](crate::Arena::try_rollback)'s docs for the
    /// full argument, which applies identically here). The same checkpoint
    /// can still be retried to finish unpublishing the rest of the suffix.
    pub fn try_rollback(&mut self, checkpoint: Checkpoint<T>) -> Result<(), CheckpointError> {
        self.validate_checkpoint(checkpoint)?;
        let target_len = checkpoint.len();
        self.identity.commit(target_len);
        while *self.published.get_mut() > target_len {
            let slot = *self.published.get_mut() - 1;
            *self.published.get_mut() = slot;
            *self.reserved.get_mut() = slot;
            let value = self
                .storage
                .take(slot)
                .expect("every published slot contains a value");
            drop(value);
        }
        Ok(())
    }

    /// Rolls back to a validated checkpoint.
    ///
    /// # Panics
    ///
    /// Panics when checkpoint validation fails.
    pub fn rollback(&mut self, checkpoint: Checkpoint<T>) {
        if let Err(error) = self.try_rollback(checkpoint) {
            panic!("invalid checkpoint: {error}");
        }
    }

    /// Removes all published values while retaining chunk allocations.
    ///
    /// # Panics
    ///
    /// A value destructor may panic. As with
    /// [`try_rollback`](Self::try_rollback), the new (empty) generation is
    /// committed before any value is taken and dropped, so a subsequent
    /// reset can continue cleanup and no stale index can validate again in
    /// the meantime.
    pub fn reset(&mut self) {
        self.identity.commit(0);
        while *self.published.get_mut() > 0 {
            let slot = *self.published.get_mut() - 1;
            *self.published.get_mut() = slot;
            *self.reserved.get_mut() = slot;
            let value = self
                .storage
                .take(slot)
                .expect("every published slot contains a value");
            drop(value);
        }
    }

    /// Returns whether `idx` is currently published by this arena.
    ///
    /// Reordered validation: `idx.stamp()` matching the identity's current
    /// stamp (one `Relaxed` load) already implies "covered by the current
    /// segment" — see `crate::segments`'s "Reordered validation" section —
    /// so the archive-table fallback only runs for a stamp that differs
    /// from it.
    #[inline]
    #[must_use]
    pub fn is_valid(&self, idx: Idx<T>) -> bool {
        idx.slot() < self.len() && self.identity.matches(idx.slot(), idx.stamp())
    }

    /// Returns the value, or `None` for a foreign, stale, or unpublished index.
    #[inline]
    #[must_use]
    pub fn try_get(&self, idx: Idx<T>) -> Option<&T> {
        if !self.is_valid(idx) {
            return None;
        }
        Some(self.storage.get(idx.slot()))
    }

    /// Iterates over the published prefix in allocation order.
    #[must_use]
    pub fn iter(&self) -> SharedArenaIter<'_, T> {
        SharedArenaIter {
            storage: &self.storage,
            pos: 0,
            len: self.len(),
        }
    }

    /// Iterates over published `(Idx<T>, &T)` pairs.
    #[must_use]
    pub fn iter_indexed(&self) -> SharedArenaIterIndexed<'_, T> {
        SharedArenaIterIndexed {
            identity: &self.identity,
            storage: &self.storage,
            pos: 0,
            len: self.len(),
        }
    }

    /// Removes and yields all values in allocation order.
    ///
    /// Slots are unpublished from the last one down to the first: each
    /// slot's `published` and `reserved` counters are decremented before
    /// that slot is taken. A panic partway through therefore leaves the
    /// slots of the prefix `[0, slot)` untouched, with both counters
    /// describing exactly that prefix, so `len()` reports it accurately and
    /// a later `drain` or `alloc` continues from a consistent state, rather
    /// than reporting an empty arena over storage that was only partially
    /// taken. The values collected before any panic are lost with the local
    /// buffer; the returned iterator, when this call completes normally,
    /// still yields every value in allocation order.
    ///
    /// # Panics
    ///
    /// Panics only if the internal invariant that every published slot is
    /// occupied has been violated. As with
    /// [`try_rollback`](Self::try_rollback)/[`reset`](Self::reset), the new
    /// (empty) generation is committed *before* any slot is taken, so every
    /// slot this call is about to unpublish is already rejected by
    /// [`is_valid`](Self::is_valid) from the moment `drain` begins —
    /// regardless of how far the take loop below has physically gotten when
    /// that invariant panic fires, and regardless of whether the caller
    /// consumes or drops the returned iterator without reading it. Without
    /// this ordering, a later `alloc` after such a panic could refill a
    /// not-yet-taken slot under the stamp a pre-drain `Idx` for it still
    /// carries, and that stale index would validate again — the same ABA
    /// hole [`Arena::try_rollback`](crate::Arena::try_rollback)'s doc
    /// comment describes for the single-thread arena.
    pub fn drain(&mut self) -> std::vec::IntoIter<T> {
        self.identity.commit(0);
        let current = *self.published.get_mut();
        let mut values = Vec::with_capacity(current);
        for slot in (0..current).rev() {
            *self.published.get_mut() = slot;
            *self.reserved.get_mut() = slot;
            let value = self
                .storage
                .take(slot)
                .expect("every published slot contains a value");
            values.push(value);
        }
        values.reverse();
        values.into_iter()
    }

    fn reserve_range(&self, len: usize) -> usize {
        assert!(len > 0, "an atomic allocation range must be non-empty");
        self.reserved
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                current.checked_add(len)
            })
            .unwrap_or_else(|_| panic!("shared arena slot space exhausted"))
    }

    fn advance_published(&self, last_slot: usize) {
        loop {
            let published = self.published.load(Ordering::Acquire);
            if published > last_slot {
                return;
            }
            if !self.storage.is_set(published) {
                std::hint::spin_loop();
                continue;
            }
            let _ = self.published.compare_exchange_weak(
                published,
                published + 1,
                Ordering::Release,
                Ordering::Relaxed,
            );
        }
    }

    /// Validates `checkpoint` against this arena's current state without
    /// changing the arena.
    ///
    /// Checks that `checkpoint` was created by this arena
    /// ([`CheckpointError::ForeignArena`] otherwise), that its length does
    /// not exceed the current published length
    /// ([`CheckpointError::BeyondCurrent`] otherwise), and that the stamp of
    /// the segment owning the slot at `checkpoint.len() - 1` still matches
    /// the stamp saved in the checkpoint ([`CheckpointError::DivergedPrefix`]
    /// otherwise). `Ok(())` means that [`rollback`](Self::rollback) or
    /// [`try_rollback`](Self::try_rollback) with this checkpoint cannot fail
    /// *validation* right now: a panicking destructor in the discarded
    /// suffix is still possible and is documented under the `# Panics`
    /// sections of those methods.
    ///
    /// A caller holding several arenas can validate every checkpoint it
    /// intends to roll back before mutating any of them, turning an
    /// otherwise independent multi-arena rollback into an all-or-nothing
    /// operation: once every checkpoint validates, none of the subsequent
    /// `rollback` calls can fail with a [`CheckpointError`]. This holds even
    /// across concurrent allocation from other threads: `alloc` and
    /// `alloc_block` only append new slots through `&self` and never touch
    /// the generation segment covering the checkpoint's boundary slot, so
    /// they cannot turn a validated checkpoint into a diverged one. Only an
    /// exclusive `&mut self` mutation on the same arena — a `rollback`,
    /// `reset`, or `drain` that truncates past the checkpoint (`drain`
    /// always truncates to zero) and lets a later allocation reuse that
    /// slot in a new generation — can invalidate a validation result taken
    /// earlier.
    ///
    /// # Errors
    ///
    /// Returns [`CheckpointError`] for a foreign checkpoint, a checkpoint
    /// beyond the current length, or a historical prefix that was replaced.
    pub fn validate_checkpoint(&self, checkpoint: Checkpoint<T>) -> Result<(), CheckpointError> {
        if self.identity.birth_raw() != checkpoint.owner().get() {
            return Err(CheckpointError::ForeignArena);
        }
        let current_len = self.len();
        if checkpoint.len() > current_len {
            return Err(CheckpointError::BeyondCurrent {
                checkpoint_len: checkpoint.len(),
                current_len,
            });
        }
        let current_tail = checkpoint
            .len()
            .checked_sub(1)
            .map(|slot| self.identity.stamp_of(slot));
        if current_tail != checkpoint.tail() {
            return Err(CheckpointError::DivergedPrefix {
                checkpoint_len: checkpoint.len(),
            });
        }
        Ok(())
    }
}

/// Out-of-line panic path for [`SharedArena::get`], kept separate so the
/// common (valid-index) branch stays small enough to inline at the call
/// site.
#[cold]
#[inline(never)]
fn unpublished_index_panic<T>(idx: Idx<T>) -> ! {
    panic!("index capability is foreign, stale, or unpublished: {idx:?}")
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

impl<'a, T> IntoIterator for &'a SharedArena<T> {
    type Item = &'a T;
    type IntoIter = SharedArenaIter<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<T> IntoIterator for SharedArena<T> {
    type Item = T;
    type IntoIter = std::vec::IntoIter<T>;

    fn into_iter(mut self) -> Self::IntoIter {
        self.drain()
    }
}

impl<T> Extend<T> for SharedArena<T> {
    fn extend<I: IntoIterator<Item = T>>(&mut self, iter: I) {
        let _ = self.alloc_block(iter);
    }
}

impl<T> std::iter::FromIterator<T> for SharedArena<T> {
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
        let arena = Self::new();
        let _ = arena.alloc_block(iter);
        arena
    }
}

/// Iterator over values in a [`SharedArena`] publication prefix.
pub struct SharedArenaIter<'a, T> {
    storage: &'a ChunkedStorage<T>,
    pos: usize,
    len: usize,
}

impl<'a, T> Iterator for SharedArenaIter<'a, T> {
    type Item = &'a T;

    fn next(&mut self) -> Option<Self::Item> {
        if self.pos == self.len {
            return None;
        }
        let value = self.storage.get(self.pos);
        self.pos += 1;
        Some(value)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.len - self.pos;
        (remaining, Some(remaining))
    }
}

impl<T> ExactSizeIterator for SharedArenaIter<'_, T> {}
impl<T> std::iter::FusedIterator for SharedArenaIter<'_, T> {}

/// Iterator over `(Idx<T>, &T)` pairs in a [`SharedArena`] prefix.
pub struct SharedArenaIterIndexed<'a, T> {
    identity: &'a SharedIdentity,
    storage: &'a ChunkedStorage<T>,
    pos: usize,
    len: usize,
}

impl<'a, T> Iterator for SharedArenaIterIndexed<'a, T> {
    type Item = (Idx<T>, &'a T);

    fn next(&mut self) -> Option<Self::Item> {
        if self.pos == self.len {
            return None;
        }
        let value = self.storage.get(self.pos);
        let idx = Idx::new(self.identity.stamp_of(self.pos), self.pos);
        self.pos += 1;
        Some((idx, value))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.len - self.pos;
        (remaining, Some(remaining))
    }
}

impl<T> ExactSizeIterator for SharedArenaIterIndexed<'_, T> {}
impl<T> std::iter::FusedIterator for SharedArenaIterIndexed<'_, T> {}
