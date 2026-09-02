use crate::stamp::Stamp;
use crate::{
    ArenaDrain, ArenaIntoIter, Block, Checkpoint, CheckpointError, Idx, IterIndexed, IterIndexedMut,
};

/// Single-thread typed arena with stamped allocation capabilities.
///
/// Values stay contiguous in a [`Vec<T>`]. A parallel stamp vector validates
/// handles without interleaving metadata with values, preserving sequential
/// traversal locality.
pub struct Arena<T> {
    owner: Stamp,
    items: Vec<T>,
    stamps: Vec<Stamp>,
}

impl<T> Arena<T> {
    /// Creates an empty arena with a fresh identity.
    #[must_use]
    pub fn new() -> Self {
        Self {
            owner: Stamp::fresh(),
            items: Vec::new(),
            stamps: Vec::new(),
        }
    }

    /// Creates an empty arena with capacity for at least `capacity` values.
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            owner: Stamp::fresh(),
            items: Vec::with_capacity(capacity),
            stamps: Vec::with_capacity(capacity),
        }
    }

    /// Allocates one value and returns its unforgeable index.
    pub fn alloc(&mut self, value: T) -> Idx<T> {
        self.reserve(1);
        let stamp = Stamp::fresh();
        let slot = self.items.len();
        self.items.push(value);
        self.stamps.push(stamp);
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

        self.reserve(len);
        let stamp = Stamp::fresh();
        let start = self.items.len();
        self.items.append(&mut values);
        self.stamps.resize(self.items.len(), stamp);
        Block::new(stamp, start, len)
    }

    /// Returns a reference to the value named by `idx`.
    ///
    /// # Panics
    ///
    /// Panics when the index belongs to another arena/allocation or has become
    /// stale after rollback, reset, or drain.
    #[must_use]
    pub fn get(&self, idx: Idx<T>) -> &T {
        self.try_get(idx)
            .unwrap_or_else(|| panic!("index capability is foreign or stale: {idx:?}"))
    }

    /// Returns a mutable reference to the value named by `idx`.
    ///
    /// # Panics
    ///
    /// Panics when the index is foreign or stale.
    #[must_use]
    pub fn get_mut(&mut self, idx: Idx<T>) -> &mut T {
        self.try_get_mut(idx)
            .unwrap_or_else(|| panic!("index capability is foreign or stale: {idx:?}"))
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

    /// Returns the number of values that can be stored without growing either
    /// the value or metadata vector.
    #[must_use]
    pub const fn capacity(&self) -> usize {
        let item_capacity = self.items.capacity();
        let stamp_capacity = self.stamps.capacity();
        if item_capacity < stamp_capacity {
            item_capacity
        } else {
            stamp_capacity
        }
    }

    /// Saves the current historical allocation prefix.
    #[must_use]
    pub fn checkpoint(&self) -> Checkpoint<T> {
        Checkpoint::new(self.owner, self.items.len(), self.stamps.last().copied())
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
    /// A destructor in the discarded suffix may panic. Each value and its
    /// stamp are removed before its destructor runs, so the arena remains
    /// structurally aligned during unwinding.
    pub fn try_rollback(&mut self, checkpoint: Checkpoint<T>) -> Result<(), CheckpointError> {
        self.validate_checkpoint(checkpoint)?;
        while self.items.len() > checkpoint.len() {
            let value = self
                .items
                .pop()
                .expect("the rollback loop observes a non-empty suffix");
            let _stamp = self
                .stamps
                .pop()
                .expect("arena value and stamp vectors have equal length");
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
    /// A value destructor may panic. The value and its stamp are removed
    /// before the destructor runs, preserving the arena invariant.
    pub fn reset(&mut self) {
        while let Some(value) = self.items.pop() {
            let _stamp = self
                .stamps
                .pop()
                .expect("arena value and stamp vectors have equal length");
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
    #[must_use]
    pub fn is_valid(&self, idx: Idx<T>) -> bool {
        self.stamps.get(idx.slot()).copied() == Some(idx.stamp())
    }

    /// Returns the referenced value, or `None` for a foreign or stale index.
    #[must_use]
    pub fn try_get(&self, idx: Idx<T>) -> Option<&T> {
        if !self.is_valid(idx) {
            return None;
        }
        self.items.get(idx.slot())
    }

    /// Returns the referenced mutable value, or `None` for a foreign or stale
    /// index.
    #[must_use]
    pub fn try_get_mut(&mut self, idx: Idx<T>) -> Option<&mut T> {
        if self.stamps.get(idx.slot()).copied() != Some(idx.stamp()) {
            return None;
        }
        self.items.get_mut(idx.slot())
    }

    /// Removes all values and yields them in allocation order.
    ///
    /// The arena becomes empty immediately; dropping this iterator drops any
    /// remaining yielded values. Capacity is retained by both backing vectors.
    pub fn drain(&mut self) -> ArenaDrain<'_, T> {
        self.stamps.clear();
        ArenaDrain::new(self.items.drain(..))
    }

    /// Iterates over `(Idx<T>, &T)` pairs in allocation order.
    #[must_use]
    pub fn iter_indexed(&self) -> IterIndexed<'_, T> {
        IterIndexed::new(self.stamps.iter(), self.items.iter())
    }

    /// Mutably iterates over `(Idx<T>, &mut T)` pairs in allocation order.
    pub fn iter_indexed_mut(&mut self) -> IterIndexedMut<'_, T> {
        IterIndexedMut::new(self.stamps.iter(), self.items.iter_mut())
    }

    /// Reserves capacity for at least `additional` more values and stamps.
    pub fn reserve(&mut self, additional: usize) {
        self.items.reserve(additional);
        self.stamps.reserve(additional);
    }

    /// Shrinks both backing vectors to fit the live prefix.
    pub fn shrink_to_fit(&mut self) {
        self.items.shrink_to_fit();
        self.stamps.shrink_to_fit();
    }

    /// Validates `checkpoint` against this arena's current state without
    /// changing the arena.
    ///
    /// Checks that `checkpoint` was created by this arena
    /// ([`CheckpointError::ForeignArena`] otherwise), that its length does
    /// not exceed the current length ([`CheckpointError::BeyondCurrent`]
    /// otherwise), and that the stamp of the slot at `checkpoint.len() - 1`
    /// still matches the stamp saved in the checkpoint
    /// ([`CheckpointError::DivergedPrefix`] otherwise). `Ok(())` means that
    /// [`rollback`](Self::rollback) or [`try_rollback`](Self::try_rollback)
    /// with this checkpoint cannot fail *validation* right now: a panicking
    /// destructor in the discarded suffix is still possible and is
    /// documented under the `# Panics` sections of those methods.
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
        if checkpoint.owner() != self.owner {
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
            .and_then(|slot| self.stamps.get(slot).copied());
        if current_tail != checkpoint.tail() {
            return Err(CheckpointError::DivergedPrefix {
                checkpoint_len: checkpoint.len(),
            });
        }
        Ok(())
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
