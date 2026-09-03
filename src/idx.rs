use std::marker::PhantomData;

use crate::stamp::Stamp;

/// Unforgeable capability naming one allocation slot.
///
/// An index is produced by [`Arena::alloc`](crate::Arena::alloc), by
/// [`Block::get`](crate::Block::get), or by an indexed arena iterator. It
/// carries the stamp of the generation segment active when the slot was
/// written, and the slot itself (see `crate::segments` for what a segment
/// is). Consequently, an index from another arena, or from an allocation
/// that a later `rollback`/`reset`/`drain` discarded and a subsequent
/// allocation overwrote, cannot alias the value that now occupies that slot.
///
/// The stamp alone does this job: `Idx` carries no separate arena-owner
/// field, because each stamp is drawn from one process-wide sequence, not a
/// per-arena one, so two independently created arenas can never coin the
/// same stamp value — a foreign stamp simply never matches.
///
/// `Idx<T>` is [`Copy`], but has no public constructor from an integer.
///
/// ```compile_fail
/// use safe_bump::Idx;
///
/// let _forged = Idx::<u8>::from_raw(0);
/// ```
pub struct Idx<T> {
    stamp: Stamp,
    slot: usize,
    marker: PhantomData<fn() -> T>,
}

impl<T> Idx<T> {
    pub(crate) const fn new(stamp: Stamp, slot: usize) -> Self {
        Self {
            stamp,
            slot,
            marker: PhantomData,
        }
    }

    pub(crate) const fn stamp(self) -> Stamp {
        self.stamp
    }

    /// Returns the absolute storage slot for diagnostics and ordering.
    ///
    /// The slot alone is not a capability and cannot be converted back into
    /// an `Idx` through the public API.
    #[must_use]
    pub const fn slot(self) -> usize {
        self.slot
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
        self.stamp == other.stamp && self.slot == other.slot
    }
}

impl<T> Eq for Idx<T> {}

impl<T> std::hash::Hash for Idx<T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.slot.hash(state);
        self.stamp.hash(state);
    }
}

impl<T> std::fmt::Debug for Idx<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Idx")
            .field("slot", &self.slot)
            .field("stamp", &self.stamp.get())
            .finish()
    }
}

impl<T> PartialOrd for Idx<T> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl<T> Ord for Idx<T> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (self.slot, self.stamp).cmp(&(other.slot, other.stamp))
    }
}
