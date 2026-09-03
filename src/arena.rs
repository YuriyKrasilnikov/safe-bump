use std::num::NonZeroU64;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::segments::Identity;
use crate::stamp::Stamp;
use crate::{
    ArenaDrain, ArenaIntoIter, Block, Checkpoint, CheckpointError, Idx, IterIndexed, IterIndexedMut,
};

/// Single-thread typed arena with stamped allocation capabilities.
///
/// Values stay contiguous in a [`Vec<T>`]. Capability validation does not
/// use a parallel per-slot metadata vector: the arena keeps one lazily
/// assigned identity instead — a permanent `birth` stamp, a `current` stamp
/// that every allocation since the last invalidating operation shares, and a
/// small table of archived segments for slots an earlier `rollback`,
/// `reset`, or `drain` left behind. See `crate::segments` for the full
/// mechanism, its compaction and ordering guarantees, and why the common
/// validation path only needs to compare a stamp against `current`.
///
/// The identity lives behind one `OnceLock<Box<Identity>>` sidecar field, so
/// an arena that never issues a capability costs only that one field beyond
/// its `Vec<T>`, and `OnceLock::get_or_init` makes the lazy `&self`
/// assignment race-free without a hand-rolled CAS protocol. `Arena`
/// additionally mirrors the raw current stamp in one inline `AtomicU64`
/// (`current`, `0` while unassigned), kept in step with the sidecar at
/// assignment and after every commit, so the common validation path reads
/// one field instead of following the `OnceLock`'s pointer into `Identity`.
pub struct Arena<T> {
    items: Vec<T>,
    identity: OnceLock<Box<Identity>>,
    current: AtomicU64,
}

impl<T> Arena<T> {
    /// Creates an empty arena. The identity is not assigned yet; it is
    /// created lazily by the first operation that hands out an `Idx`,
    /// `Block`, or `Checkpoint`.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            items: Vec::new(),
            identity: OnceLock::new(),
            current: AtomicU64::new(0),
        }
    }

    /// Creates an empty arena with capacity for at least `capacity` values.
    /// The identity is not assigned yet; see [`new`](Self::new).
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            items: Vec::with_capacity(capacity),
            identity: OnceLock::new(),
            current: AtomicU64::new(0),
        }
    }

    /// Returns this arena's identity, assigning one on the first call.
    #[inline]
    fn identity(&self) -> &Identity {
        self.identity.get_or_init(|| {
            let identity = Box::new(Identity::new(Stamp::fresh()));
            self.current
                .store(identity.current().get(), Ordering::Relaxed);
            identity
        })
    }

    /// Returns this arena's identity without forcing assignment. Used by the
    /// cold validation path, which only ever needs to *compare* against an
    /// existing identity, never mint one: an arena with no identity yet has
    /// no capability that could compare equal, and also has no items, so
    /// [`is_valid`](Self::is_valid) is always rejected by the length check
    /// first.
    #[inline]
    fn identity_opt(&self) -> Option<&Identity> {
        self.identity.get().map(Box::as_ref)
    }

    /// Returns the stamp a new allocation receives right now. The common
    /// case (an identity already assigned) reads the inline mirror directly
    /// — one `Relaxed` load and a nonzero test — without following the
    /// `OnceLock`'s pointer into `Identity`; only the arena's very first
    /// capability takes the cold path through [`identity`](Self::identity),
    /// which assigns the identity and publishes the mirror.
    #[inline]
    fn current_stamp(&self) -> Stamp {
        NonZeroU64::new(self.current.load(Ordering::Relaxed))
            .map_or_else(|| self.identity().current(), Stamp::from_nonzero)
    }

    /// Allocates one value and returns its unforgeable index.
    pub fn alloc(&mut self, value: T) -> Idx<T> {
        let stamp = self.current_stamp();
        let slot = self.items.len();
        self.items.push(value);
        Idx::new(stamp, slot)
    }

    /// Allocates one contiguous batch and returns its block capability.
    ///
    /// The input is fully collected before arena state changes. If iteration
    /// panics, the arena therefore retains its original prefix. This
    /// guarantee covers the whole arena, not just this call's own effects,
    /// which is stronger than what the `experimental-shared` arena can
    /// promise for its equivalent method: this method borrows `self`
    /// exclusively for its entire duration, so the borrow checker statically
    /// excludes a reentrant caller-supplied iterator from holding another
    /// reference to this same arena and mutating it before panicking.
    pub fn alloc_block(&mut self, iter: impl IntoIterator<Item = T>) -> Block<T> {
        let mut values: Vec<T> = iter.into_iter().collect();
        let len = values.len();
        if len == 0 {
            return Block::empty();
        }

        let stamp = self.current_stamp();
        let start = self.items.len();
        self.items.append(&mut values);
        Block::new(stamp, start, len)
    }

    /// Returns a reference to the value named by `idx`.
    ///
    /// # Panics
    ///
    /// Panics when the index belongs to another arena/allocation or has become
    /// stale after rollback, reset, or drain.
    #[inline]
    #[must_use]
    pub fn get(&self, idx: Idx<T>) -> &T {
        self.try_get(idx).unwrap_or_else(|| stale_index_panic(idx))
    }

    /// Returns a mutable reference to the value named by `idx`.
    ///
    /// # Panics
    ///
    /// Panics when the index is foreign or stale.
    #[inline]
    #[must_use]
    pub fn get_mut(&mut self, idx: Idx<T>) -> &mut T {
        self.try_get_mut(idx)
            .unwrap_or_else(|| stale_index_panic(idx))
    }

    /// Returns the number of live values.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.items.len()
    }

    /// Returns `true` when the arena contains no live values.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Returns the number of values that can be stored without growing the
    /// backing vector.
    #[must_use]
    pub const fn capacity(&self) -> usize {
        self.items.capacity()
    }

    /// Saves the current historical allocation prefix.
    #[must_use]
    pub fn checkpoint(&self) -> Checkpoint<T> {
        let identity = self.identity();
        let len = self.items.len();
        let tail = len.checked_sub(1).map(|slot| identity.stamp_of(slot));
        Checkpoint::new(identity.birth(), len, tail)
    }

    /// Validates and rolls back to `checkpoint`.
    ///
    /// Values after the saved prefix are dropped from the highest slot toward
    /// the prefix. The arena is unchanged when validation fails.
    ///
    /// # Errors
    ///
    /// Returns [`CheckpointError`] for a foreign checkpoint, a checkpoint
    /// beyond the current length, or a historical prefix that was replaced.
    ///
    /// # Panics
    ///
    /// A destructor in the discarded suffix may panic. The new segment
    /// boundary is committed *before* any value is dropped, so every slot
    /// `>= checkpoint.len()` is already rejected by
    /// [`is_valid`](Self::is_valid) from the moment this call begins —
    /// regardless of how far the drop loop below has physically gotten when
    /// a destructor panics. This closes an ABA hole a commit-after-the-loop
    /// ordering would have: without it, an `alloc` issued after a caught
    /// panic (instead of retrying this call) could reuse an unpopped slot
    /// under the same segment stamp a stale `Idx` for it still carried, and
    /// that stale index would validate again. The same checkpoint can still
    /// be retried to finish dropping the rest of the suffix; retrying after
    /// the boundary already committed is safe (see `crate::segments`'s
    /// module documentation).
    pub fn try_rollback(&mut self, checkpoint: Checkpoint<T>) -> Result<(), CheckpointError> {
        self.validate_checkpoint(checkpoint)?;
        let target_len = checkpoint.len();
        let identity = self
            .identity
            .get_mut()
            .expect("a validated checkpoint implies this arena already has an identity");
        identity.commit(target_len);
        self.current
            .store(identity.current().get(), Ordering::Relaxed);
        while self.items.len() > target_len {
            let value = self
                .items
                .pop()
                .expect("the rollback loop observes a non-empty suffix");
            drop(value);
        }
        Ok(())
    }

    /// Rolls back to a validated checkpoint.
    ///
    /// # Panics
    ///
    /// Panics when the checkpoint belongs to another arena, extends beyond the
    /// current state, or names a prefix that was discarded and replaced.
    pub fn rollback(&mut self, checkpoint: Checkpoint<T>) {
        if let Err(error) = self.try_rollback(checkpoint) {
            panic!("invalid checkpoint: {error}");
        }
    }

    /// Removes all values while retaining allocated capacity.
    ///
    /// # Panics
    ///
    /// A value destructor may panic. As with [`try_rollback`](Self::try_rollback),
    /// the new (empty) generation is committed *before* any value is
    /// dropped, so every existing index — including ones for values not yet
    /// physically popped when a destructor panics — is already rejected by
    /// [`is_valid`](Self::is_valid) from the moment this call begins. The
    /// same reset can be retried to finish dropping the rest of the arena.
    pub fn reset(&mut self) {
        if let Some(identity) = self.identity.get_mut() {
            identity.commit(0);
            self.current
                .store(identity.current().get(), Ordering::Relaxed);
        }
        while let Some(value) = self.items.pop() {
            drop(value);
        }
    }

    /// Returns an iterator over values in allocation order.
    pub fn iter(&self) -> std::slice::Iter<'_, T> {
        self.items.iter()
    }

    /// Returns a mutable iterator over values in allocation order.
    pub fn iter_mut(&mut self) -> std::slice::IterMut<'_, T> {
        self.items.iter_mut()
    }

    /// Returns whether `idx` is currently valid for this arena.
    #[inline]
    #[must_use]
    pub fn is_valid(&self, idx: Idx<T>) -> bool {
        idx.slot() < self.items.len() && self.stamp_matches(idx)
    }

    /// Reordered validation: `idx.stamp()` matching the inline mirror is
    /// checked first (one `Relaxed` load) and already implies "covered by
    /// the current segment" — see `crate::segments`'s "Reordered validation"
    /// section — so the archive-table fallback only runs for a stamp that
    /// differs from the mirror.
    #[inline]
    fn stamp_matches(&self, idx: Idx<T>) -> bool {
        idx.stamp().get() == self.current.load(Ordering::Relaxed)
            || self.stamp_matches_archived(idx)
    }

    #[cold]
    #[inline(never)]
    fn stamp_matches_archived(&self, idx: Idx<T>) -> bool {
        self.identity_opt()
            .is_some_and(|identity| identity.matches_archived(idx.slot(), idx.stamp()))
    }

    /// Returns the referenced value, or `None` for a foreign or stale index.
    #[inline]
    #[must_use]
    pub fn try_get(&self, idx: Idx<T>) -> Option<&T> {
        let value = self.items.get(idx.slot())?;
        self.stamp_matches(idx).then_some(value)
    }

    /// Returns the referenced mutable value, or `None` for a foreign or stale
    /// index.
    #[inline]
    #[must_use]
    pub fn try_get_mut(&mut self, idx: Idx<T>) -> Option<&mut T> {
        if !self.stamp_matches(idx) {
            return None;
        }
        self.items.get_mut(idx.slot())
    }

    /// Removes all values and yields them in allocation order.
    ///
    /// The arena becomes empty immediately; dropping this iterator drops any
    /// remaining yielded values. Capacity is retained by the backing vector.
    ///
    /// Unlike [`try_rollback`](Self::try_rollback)/[`reset`](Self::reset),
    /// this never needed reordering for the same reason: the new generation
    /// is already committed here, before [`Vec::drain`] even runs — no value
    /// is dropped until the caller consumes or drops the returned iterator,
    /// which happens after this call has already returned.
    pub fn drain(&mut self) -> ArenaDrain<'_, T> {
        if let Some(identity) = self.identity.get_mut() {
            identity.commit(0);
            self.current
                .store(identity.current().get(), Ordering::Relaxed);
        }
        ArenaDrain::new(self.items.drain(..))
    }

    /// Iterates over `(Idx<T>, &T)` pairs in allocation order.
    #[must_use]
    pub fn iter_indexed(&self) -> IterIndexed<'_, T> {
        IterIndexed::new(self.identity_opt(), self.items.iter())
    }

    /// Mutably iterates over `(Idx<T>, &mut T)` pairs in allocation order.
    pub fn iter_indexed_mut(&mut self) -> IterIndexedMut<'_, T> {
        IterIndexedMut::new(self.identity.get().map(Box::as_ref), self.items.iter_mut())
    }

    /// Reserves capacity for at least `additional` more values.
    pub fn reserve(&mut self, additional: usize) {
        self.items.reserve(additional);
    }

    /// Shrinks the backing vector to fit the live prefix.
    pub fn shrink_to_fit(&mut self) {
        self.items.shrink_to_fit();
    }

    /// Validates `checkpoint` against this arena's current state without
    /// changing the arena.
    ///
    /// Checks that `checkpoint` was created by this arena
    /// ([`CheckpointError::ForeignArena`] otherwise), that its length does
    /// not exceed the current length ([`CheckpointError::BeyondCurrent`]
    /// otherwise), and that the stamp of the segment owning the slot at
    /// `checkpoint.len() - 1` still matches the stamp saved in the
    /// checkpoint ([`CheckpointError::DivergedPrefix`] otherwise). `Ok(())`
    /// means that [`rollback`](Self::rollback) or
    /// [`try_rollback`](Self::try_rollback) with this checkpoint cannot fail
    /// *validation* right now: a panicking destructor in the discarded
    /// suffix is still possible and is documented under the `# Panics`
    /// sections of those methods.
    ///
    /// A caller holding several arenas can validate every checkpoint it
    /// intends to roll back before mutating any of them, turning an
    /// otherwise independent multi-arena rollback into an all-or-nothing
    /// operation: once every checkpoint validates, none of the subsequent
    /// `rollback` calls can fail with a [`CheckpointError`].
    ///
    /// # Errors
    ///
    /// Returns [`CheckpointError`] for a foreign checkpoint, a checkpoint
    /// beyond the current length, or a historical prefix that was replaced.
    pub fn validate_checkpoint(&self, checkpoint: Checkpoint<T>) -> Result<(), CheckpointError> {
        let Some(identity) = self.identity_opt() else {
            // No identity has ever been assigned: this arena has never
            // returned a checkpoint of its own, so any checkpoint object at
            // all — even an empty one — must have come from elsewhere.
            return Err(CheckpointError::ForeignArena);
        };
        if identity.birth() != checkpoint.owner() {
            return Err(CheckpointError::ForeignArena);
        }
        if checkpoint.len() > self.len() {
            return Err(CheckpointError::BeyondCurrent {
                checkpoint_len: checkpoint.len(),
                current_len: self.len(),
            });
        }
        let current_tail = checkpoint
            .len()
            .checked_sub(1)
            .map(|slot| identity.stamp_of(slot));
        if current_tail != checkpoint.tail() {
            return Err(CheckpointError::DivergedPrefix {
                checkpoint_len: checkpoint.len(),
            });
        }
        Ok(())
    }
}

/// Out-of-line panic path for [`Arena::get`]/[`Arena::get_mut`], kept
/// separate so the common (valid-index) branch of each stays small enough to
/// inline at the call site.
#[cold]
#[inline(never)]
fn stale_index_panic<T>(idx: Idx<T>) -> ! {
    panic!("index capability is foreign or stale: {idx:?}")
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
        let _ = self.alloc_block(iter);
    }
}

impl<T> std::iter::FromIterator<T> for Arena<T> {
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
        let mut arena = Self::new();
        let _ = arena.alloc_block(iter);
        arena
    }
}

impl<T> IntoIterator for Arena<T> {
    type Item = T;
    type IntoIter = ArenaIntoIter<T>;

    fn into_iter(self) -> Self::IntoIter {
        ArenaIntoIter::new(self.items.into_iter())
    }
}
