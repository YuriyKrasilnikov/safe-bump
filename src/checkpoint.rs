use std::marker::PhantomData;

use crate::stamp::Stamp;

/// Saved allocation prefix for validated rollback.
///
/// Checkpoints can only be created by an arena. Besides the prefix length,
/// they carry the arena's permanent birth identity and the stamp of the
/// generation segment that owned the prefix's tail slot at capture time.
/// This rejects foreign checkpoints and the equal-length ABA case after a
/// prefix was discarded and replaced.
///
/// `owner` is the arena's *birth* stamp — the one stamp assigned once, on
/// the first capability the arena ever issues, and never touched again —
/// not the segment stamp active at capture time (that is `tail`). Using the
/// permanent identity here, rather than the live "current" stamp, is what
/// lets a foreign *empty* checkpoint (`len == 0`) still be rejected as
/// [`CheckpointError::ForeignArena`], and an *own* empty checkpoint captured
/// after a `reset` still be accepted: both compare `owner` against the
/// target arena's birth stamp, which a `reset` never changes, rather than
/// against a `current` stamp that a `reset` always does change.
///
/// `tail` is `None` exactly when `len == 0` — an empty prefix has no tail
/// slot to diverge, so an empty-prefix checkpoint always validates against
/// any current state of its own arena. Because `Stamp` is a `NonZeroU64`,
/// `Option<Stamp>` costs no extra space over `Stamp` itself: the compiler
/// folds the discriminant into the niche `0` leaves unused.
pub struct Checkpoint<T> {
    owner: Stamp,
    len: usize,
    tail: Option<Stamp>,
    marker: PhantomData<fn() -> T>,
}

impl<T> Checkpoint<T> {
    pub(crate) const fn new(owner: Stamp, len: usize, tail: Option<Stamp>) -> Self {
        Self {
            owner,
            len,
            tail,
            marker: PhantomData,
        }
    }

    pub(crate) const fn owner(self) -> Stamp {
        self.owner
    }

    pub(crate) const fn tail(self) -> Option<Stamp> {
        self.tail
    }

    /// Returns the saved prefix length.
    #[must_use]
    pub const fn len(self) -> usize {
        self.len
    }

    /// Returns `true` when the saved prefix is empty.
    #[must_use]
    pub const fn is_empty(self) -> bool {
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
        self.owner == other.owner && self.len == other.len && self.tail == other.tail
    }
}

impl<T> Eq for Checkpoint<T> {}

impl<T> std::hash::Hash for Checkpoint<T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.owner.hash(state);
        self.len.hash(state);
        self.tail.hash(state);
    }
}

impl<T> std::fmt::Debug for Checkpoint<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Checkpoint")
            .field("owner", &self.owner.get())
            .field("len", &self.len)
            .field("tail", &self.tail.map(Stamp::get))
            .finish()
    }
}

impl<T> PartialOrd for Checkpoint<T> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl<T> Ord for Checkpoint<T> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (self.owner, self.len, self.tail).cmp(&(other.owner, other.len, other.tail))
    }
}

/// Reason a checkpoint cannot be applied to the current allocation prefix.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CheckpointError {
    /// The checkpoint was created by a different arena.
    ForeignArena,
    /// The checkpoint names a prefix longer than the current allocation state.
    BeyondCurrent {
        /// Saved checkpoint length.
        checkpoint_len: usize,
        /// Current arena length.
        current_len: usize,
    },
    /// The slot at the saved boundary was discarded and later replaced.
    DivergedPrefix {
        /// Saved checkpoint length.
        checkpoint_len: usize,
    },
}

impl std::fmt::Display for CheckpointError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ForeignArena => f.write_str("checkpoint belongs to a different arena"),
            Self::BeyondCurrent {
                checkpoint_len,
                current_len,
            } => write!(
                f,
                "checkpoint length {checkpoint_len} exceeds current length {current_len}"
            ),
            Self::DivergedPrefix { checkpoint_len } => write!(
                f,
                "checkpoint prefix of length {checkpoint_len} has been replaced"
            ),
        }
    }
}

impl std::error::Error for CheckpointError {}
