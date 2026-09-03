use std::marker::PhantomData;

use crate::Idx;
use crate::stamp::Stamp;

/// Capability for one contiguous batch allocation.
///
/// Every non-empty block owns a half-open slot interval `[start, start + len)`
/// under one generation-segment stamp. [`get`](Self::get) is the only public
/// operation that derives an offset index, so an index cannot escape the
/// interval it proves.
pub struct Block<T> {
    stamp: Option<Stamp>,
    start: usize,
    len: usize,
    marker: PhantomData<fn() -> T>,
}

impl<T> Block<T> {
    pub(crate) const fn empty() -> Self {
        Self {
            stamp: None,
            start: 0,
            len: 0,
            marker: PhantomData,
        }
    }

    pub(crate) fn new(stamp: Stamp, start: usize, len: usize) -> Self {
        assert!(len > 0, "a stamped allocation block must be non-empty");
        start
            .checked_add(len - 1)
            .expect("allocation block end exceeds usize");
        Self {
            stamp: Some(stamp),
            start,
            len,
            marker: PhantomData,
        }
    }

    /// Returns the number of indices in the block.
    #[must_use]
    pub const fn len(self) -> usize {
        self.len
    }

    /// Returns `true` when the allocation iterator yielded no values.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.len == 0
    }

    /// Returns the first index, or `None` for an empty block.
    #[must_use]
    pub const fn first(self) -> Option<Idx<T>> {
        match self.stamp {
            Some(stamp) => Some(Idx::new(stamp, self.start)),
            None => None,
        }
    }

    /// Returns the last index, or `None` for an empty block.
    #[must_use]
    pub const fn last(self) -> Option<Idx<T>> {
        self.get(self.len.saturating_sub(1))
    }

    /// Derives the index at `offset`, or returns `None` outside the block.
    #[must_use]
    pub const fn get(self, offset: usize) -> Option<Idx<T>> {
        if offset >= self.len {
            return None;
        }
        let Some(stamp) = self.stamp else {
            return None;
        };
        let Some(slot) = self.start.checked_add(offset) else {
            return None;
        };
        Some(Idx::new(stamp, slot))
    }

    /// Returns `true` when `idx` is one of the capabilities in this block.
    #[must_use]
    pub fn contains(self, idx: Idx<T>) -> bool {
        self.stamp.is_some_and(|stamp| {
            idx.stamp() == stamp && idx.slot() >= self.start && idx.slot() - self.start < self.len
        })
    }

    /// Iterates over all indices in increasing slot order.
    #[must_use]
    pub const fn indices(self) -> BlockIndices<T> {
        BlockIndices {
            block: self,
            front: 0,
            back: self.len,
        }
    }
}

impl<T> Clone for Block<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for Block<T> {}

impl<T> PartialEq for Block<T> {
    fn eq(&self, other: &Self) -> bool {
        self.stamp == other.stamp && self.start == other.start && self.len == other.len
    }
}

impl<T> Eq for Block<T> {}

impl<T> std::hash::Hash for Block<T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.stamp.hash(state);
        self.start.hash(state);
        self.len.hash(state);
    }
}

impl<T> std::fmt::Debug for Block<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Block")
            .field("stamp", &self.stamp.map(Stamp::get))
            .field("start", &self.start)
            .field("len", &self.len)
            .finish()
    }
}

/// Exact-size iterator over the capabilities in a [`Block`].
pub struct BlockIndices<T> {
    block: Block<T>,
    front: usize,
    back: usize,
}

impl<T> Iterator for BlockIndices<T> {
    type Item = Idx<T>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.front == self.back {
            return None;
        }
        let offset = self.front;
        self.front += 1;
        self.block.get(offset)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.back - self.front;
        (remaining, Some(remaining))
    }
}

impl<T> DoubleEndedIterator for BlockIndices<T> {
    fn next_back(&mut self) -> Option<Self::Item> {
        if self.front == self.back {
            return None;
        }
        self.back -= 1;
        self.block.get(self.back)
    }
}

impl<T> ExactSizeIterator for BlockIndices<T> {}
impl<T> std::iter::FusedIterator for BlockIndices<T> {}
