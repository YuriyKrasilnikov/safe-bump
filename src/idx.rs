use std::marker::PhantomData;

/// Stable index into an [`Arena`](crate::Arena) or
/// [`SharedArena`](crate::SharedArena).
///
/// Obtained from [`Arena::alloc`](crate::Arena::alloc) or
/// [`SharedArena::alloc`](crate::SharedArena::alloc). Implements [`Copy`],
/// so it can be freely duplicated and stored in data structures.
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
