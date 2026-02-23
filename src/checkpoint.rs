use std::marker::PhantomData;

/// Saved allocation state for rollback.
///
/// Created by [`Arena::checkpoint`](crate::Arena::checkpoint) or
/// [`SharedArena::checkpoint`](crate::SharedArena::checkpoint). Rolling back
/// to a checkpoint drops all values allocated after it and retains everything
/// before.
pub struct Checkpoint<T> {
    len: usize,
    _marker: PhantomData<T>,
}

impl<T> Checkpoint<T> {
    /// Creates a checkpoint from a saved length.
    ///
    /// The caller must ensure the length is valid for the target arena.
    #[must_use]
    pub const fn from_len(len: usize) -> Self {
        Self {
            len,
            _marker: PhantomData,
        }
    }

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
