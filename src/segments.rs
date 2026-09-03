//! Per-arena identity bookkeeping shared by [`Arena`](crate::Arena) and
//! [`SharedArena`](crate::SharedArena).
//!
//! # The generation is a fresh process-unique stamp
//!
//! Each arena keeps one *identity*: a permanent `birth` [`Stamp`], a
//! `current` stamp that every ongoing allocation receives, and a small table
//! of archived `(start, stamp)` segments for slots that predate `current`.
//! `birth` and `current` start out equal — literally the same value, not
//! independently drawn — because the identity is created lazily by the
//! *first* capability an arena ever issues ([`Arena::alloc`](crate::Arena::alloc),
//! [`Arena::alloc_block`](crate::Arena::alloc_block), or
//! [`Arena::checkpoint`](crate::Arena::checkpoint)), and that first
//! allocation defines both the arena's permanent identity and its current
//! segment at once.
//!
//! Every invalidating operation (`try_rollback`/`rollback`, `reset`,
//! `drain`) archives the segment that has been current so far and draws a
//! *fresh* [`Stamp::fresh`] for the new one. Because `Stamp::fresh` is
//! unique across the whole process — not just within one arena — a slot's
//! stamp already encodes "this arena, this epoch" on its own: no separate
//! owner field is needed to reject a foreign index, only a single 64-bit
//! comparison against the stamp of the segment the slot belongs to. This is
//! why [`Idx`](crate::Idx) needs only a stamp and a slot: validating it is
//! one comparison in the common case, not a comparison against a per-arena
//! owner plus a separate generation counter.
//!
//! `birth` is what [`Checkpoint`](crate::Checkpoint) carries as its
//! permanent arena identity: it must survive every `current` change so that
//! a foreign *empty* checkpoint is still rejected, and an *own* empty
//! checkpoint (captured after a `reset`) is still accepted — see
//! [`Arena::validate_checkpoint`](crate::Arena::validate_checkpoint)'s doc
//! comment for why comparing against `birth` rather than the live `current`
//! is the rule that preserves "prefix identical".
//!
//! # Table layout, compaction, and commit-before-drop
//!
//! - The *current* (most recently committed) segment's stamp and start slot
//!   are cached inline. Looking up the stamp covering a slot `>=
//!   current_start` — the overwhelming majority of calls, since most arenas
//!   are never rolled back — is a single field compare with no table
//!   access. Only a slot belonging to an *older*, superseded segment falls
//!   through to the cold path, which binary-searches a table of archived
//!   segments. That table stays empty until the *second* invalidating
//!   operation: the first is already fully described by `birth` (the
//!   fallback), so nothing needs archiving yet.
//! - A new segment boundary is committed *before* any value in the
//!   invalidated range is dropped (see
//!   [`Arena::try_rollback`](crate::Arena::try_rollback) and
//!   [`Arena::reset`](crate::Arena::reset)), not after. A destructor that
//!   panics partway through a drop loop must not leave a not-yet-popped slot
//!   still reporting its *old* stamp: if it did, and the caller allocated
//!   directly instead of retrying, the new allocation could reuse that slot
//!   under a generation a stale `Idx` for it still carries, and the stale
//!   index would validate again. Committing first closes this hole
//!   regardless of how far a drop loop has physically gotten, and regardless
//!   of whether the caller ever retries.
//! - Because a commit can run more than once for the same logical operation
//!   (a retried invalidating call after a panic), the same `(start, stamp)`
//!   boundary can be archived more than once. Compaction turns those into
//!   cheap dead entries: before appending the boundary about to become
//!   historical, every existing table entry whose own `start` is `>=` that
//!   boundary's start is dropped, because such an entry can never again be
//!   the covering segment for any slot — the entry about to be pushed is
//!   strictly more recent and already wins every comparison the dropped
//!   entry could have won. This bounds the table at one entry per distinct
//!   `start`, keeps it sorted by `start` ascending (so the cold path
//!   binary-searches instead of scanning), and bounds its length by the
//!   number of invalidating operations the arena has ever survived.
//!
//! # Reordered validation
//!
//! An index whose stamp equals `current` was necessarily minted after the
//! last commit — a commit always draws a fresh stamp and sets
//! `current_start` to the post-commit length *before* any later allocation
//! can reuse a slot in that range, and stamps are never repeated — so
//! `stamp == current` already implies "this slot is covered by the current
//! segment" without needing to also compare `slot` against `current_start`.
//! Both identity types test the stamp first and only fall back to the
//! `current_start`/archive-table check
//! ([`Identity::matches_archived`]/[`SharedIdentity::matches_archived`]) for
//! a stamp that differs from `current` — exactly the same cold path as
//! before, just reached one comparison later on the common (matching) path.
//! `SharedIdentity::matches` does both steps itself (its `current` is
//! already the single `AtomicU64` a caller would compare against); `Arena`
//! does the fast comparison against its own inline mirror directly, so
//! `Identity` only needs to expose the cold fallback.
//!
//! # Layout
//!
//! [`Arena`](crate::Arena) stores the identity behind one
//! `OnceLock<Box<Identity>>` sidecar field, so an arena that never issues a
//! capability costs only that one field beyond its `Vec<T>`, and lazily
//! assigning it through a shared reference is race-free by construction
//! (`OnceLock::get_or_init`). `Arena` additionally mirrors `identity`'s raw
//! current stamp in one inline `AtomicU64` (`0` while unassigned), written
//! once at assignment and again after every commit: the common validation
//! path reads that single field with a `Relaxed` load instead of chasing the
//! `OnceLock`'s pointer into `Identity`, and only a stamp that does not
//! match the mirror falls back to the sidecar for the cold, archived-segment
//! path. [`SharedIdentity`] needs no separate mirror: its own `current` is
//! already a directly-held `AtomicU64`.

#[cfg(feature = "experimental-shared")]
use std::sync::atomic::{AtomicU64, Ordering};

use crate::stamp::Stamp;

/// Single-thread per-arena identity, used by [`Arena`](crate::Arena).
///
/// Lazily created (behind `Arena`'s `OnceLock<Box<Identity>>`) on the first
/// call that hands out an `Idx`, `Block`, or `Checkpoint`. See the module
/// documentation for `birth`/`current`/`table`'s roles.
pub struct Identity {
    birth: Stamp,
    current: Stamp,
    current_start: usize,
    /// Stamp covering slot `current_start - 1` — the slot immediately below
    /// the current segment. Meaningful only when `current_start > 0`;
    /// maintained by [`commit`](Self::commit) so [`stamp_of`](Self::stamp_of)
    /// can answer that one slot in O(1) instead of falling into
    /// [`stamp_of_cold`](Self::stamp_of_cold)'s binary search — which is
    /// exactly the slot `checkpoint`/`validate_checkpoint` ask about right
    /// after a `rollback`/`reset`/`drain`.
    prefix_tail: Stamp,
    table: Vec<(usize, Stamp)>,
}

impl Identity {
    /// Creates the identity born by `stamp`: `birth` and `current` start out
    /// equal (this *is* the first segment, not merely equal to it).
    pub(crate) const fn new(stamp: Stamp) -> Self {
        Self {
            birth: stamp,
            current: stamp,
            current_start: 0,
            // Not yet meaningful: `current_start == 0`, so `stamp_of` never
            // reads this before the first `commit`.
            prefix_tail: stamp,
            table: Vec::new(),
        }
    }

    /// Returns the permanent arena identity, fixed for this identity's whole
    /// lifetime.
    #[inline]
    pub(crate) const fn birth(&self) -> Stamp {
        self.birth
    }

    /// Returns the stamp that a new allocation receives right now.
    #[inline]
    pub(crate) const fn current(&self) -> Stamp {
        self.current
    }

    /// Returns the stamp that currently covers `slot`.
    #[inline]
    pub(crate) fn stamp_of(&self, slot: usize) -> Stamp {
        if slot >= self.current_start {
            self.current
        } else if slot + 1 == self.current_start {
            // The one slot immediately below the current segment: answered
            // from the cached `prefix_tail` instead of `stamp_of_cold`'s
            // binary search. `current_start > 0` is implied here (the first
            // branch above already covers every slot when it is `0`), so
            // `prefix_tail` is guaranteed meaningful.
            self.prefix_tail
        } else {
            self.stamp_of_cold(slot)
        }
    }

    /// Cold path of the reordered validation (see the module documentation's
    /// "Reordered validation" section): `stamp` is not the current segment's
    /// mirrored value, so it can only be valid through the archived table
    /// (and only for a slot that predates the current segment). `Arena`
    /// calls this directly after its own inline-mirror comparison misses.
    #[cold]
    #[inline(never)]
    pub(crate) fn matches_archived(&self, slot: usize, stamp: Stamp) -> bool {
        slot < self.current_start && self.stamp_of_cold(slot) == stamp
    }

    /// Cold path: `slot` predates the current segment, so it can only be
    /// covered by an archived one, or by none — meaning the implicit birth
    /// segment, which is never itself archived (see the module doc comment).
    /// The table is kept sorted by `start` ascending, so the covering entry
    /// — the rightmost one with `start <= slot` — is found by binary search.
    #[cold]
    #[inline(never)]
    fn stamp_of_cold(&self, slot: usize) -> Stamp {
        let covering = self.table.partition_point(|(start, _)| *start <= slot);
        covering
            .checked_sub(1)
            .map_or(self.birth, |index| self.table[index].1)
    }

    /// Commits a new segment boundary: from this call onward, every slot
    /// `>= new_len` belongs to a freshly drawn stamp. Callers invoke this
    /// *before* dropping any value in the invalidated range — see the module
    /// documentation for why the ordering, not just the eventual effect, is
    /// load-bearing. Calling this more than once for the same logical
    /// operation (a retried invalidating call) is safe: compaction keeps the
    /// table from growing without bound.
    pub(crate) fn commit(&mut self, new_len: usize) {
        let old_start = self.current_start;
        // Archive the segment that has been "current" so far, unless it is
        // still the birth segment (no commit has happened on this identity
        // yet), which is never itself archived — the cold path's fallback
        // already recovers it. `current != birth` is an exact test, not a
        // heuristic: stamps are drawn from a process-wide sequence that
        // never repeats a value, so `current` can equal `birth` only while
        // literally still holding the value it was created with.
        let old_current = (self.current != self.birth).then_some(self.current);
        if let Some(old_current) = old_current {
            let dead_from = self
                .table
                .partition_point(|(start, _)| *start < self.current_start);
            self.table.truncate(dead_from);
            self.table.push((self.current_start, old_current));
        }
        self.current = Stamp::fresh();
        self.current_start = new_len;

        // Keep `prefix_tail` — the stamp covering slot `new_len - 1`, the
        // slot immediately below the new current segment — an O(1) cache of
        // what `stamp_of(new_len - 1)` would return.
        self.prefix_tail = match old_current {
            // `new_len == 0`: slot `new_len - 1` does not exist, and
            // `stamp_of`/`checkpoint` never read `prefix_tail` while
            // `current_start == 0`. Leave the field as-is; there is nothing
            // cheaper than not computing it at all.
            _ if new_len == 0 => self.prefix_tail,
            // Slot `new_len - 1 >= old_start` falls inside the segment just
            // archived above as `(old_start, old_current)`: that entry is
            // now its covering one, so the answer is already in hand. This
            // is also the only way the table ever *grows*: each commit that
            // extends it targets a `new_len` strictly past the segment it
            // just archived, which is exactly this branch — a commit that
            // instead reaches back past `old_start` (the next branch) never
            // adds a new distinct entry, it can only get compacted away.
            Some(old_current) if new_len > old_start => old_current,
            // `new_len <= old_start`: this commit reaches back past the
            // segment just archived, into one that was already historical
            // before this call. The entry just pushed cannot cover slot
            // `new_len - 1` (its `start` is `old_start`, which is `>
            // new_len - 1` here), so there is no O(1) answer available —
            // fall back to the same search `stamp_of_cold` uses. `table`
            // already reflects this call's archiving, but that is harmless:
            // the pushed entry is never the one the search selects.
            Some(_) => self.stamp_of_cold(new_len - 1),
            // `old_current` is `None`: `current` still equalled `birth` on
            // entry, so this is the very first commit this identity has
            // ever seen (see the comment above `old_current`'s definition).
            // Nothing is archived yet, and slot `new_len - 1` (which exists
            // because `new_len > 0` here) was allocated before this call,
            // i.e. under `birth`.
            None => self.birth,
        };
    }
}

/// Concurrency-friendly per-arena identity, used by
/// [`SharedArena`](crate::SharedArena).
///
/// `birth` and `current` are [`AtomicU64`] (raw stamp values, `0` meaning
/// "not yet assigned") so [`SharedArena::alloc`](crate::SharedArena::alloc)
/// and [`SharedArena::is_valid`](crate::SharedArena::is_valid) can read them
/// through a shared reference. `current_start` and the archive table are
/// plain (non-atomic) fields: they are only ever written by the `&mut self`
/// operations (`rollback`/`reset`/`drain`), which the borrow checker
/// guarantees cannot overlap with any concurrently running `&self` call
/// (allocation or validation) — so no synchronization is needed for them,
/// only for the two scalars `&self` methods read on their own.
#[cfg(feature = "experimental-shared")]
pub struct SharedIdentity {
    birth: AtomicU64,
    current: AtomicU64,
    current_start: usize,
    /// Stamp covering slot `current_start - 1`, the slot immediately below
    /// the current segment. Meaningful only when `current_start > 0`; like
    /// `current_start` and `table`, it is only ever written by the `&mut
    /// self` operations (see the struct doc comment), so no synchronization
    /// is needed for it either. Maintained by [`commit`](Self::commit) so
    /// [`stamp_of`](Self::stamp_of) can answer that one slot in O(1) instead
    /// of falling into [`stamp_of_cold`](Self::stamp_of_cold)'s binary
    /// search.
    prefix_tail: Stamp,
    #[allow(clippy::box_collection)]
    table: Option<Box<Vec<(usize, Stamp)>>>,
}

#[cfg(feature = "experimental-shared")]
impl SharedIdentity {
    pub(crate) const fn new() -> Self {
        Self {
            birth: AtomicU64::new(0),
            current: AtomicU64::new(0),
            current_start: 0,
            // Not yet meaningful (`current_start == 0`, `birth`/`current`
            // themselves still unassigned); any nonzero value works as the
            // placeholder since nothing reads it before the first `commit`.
            prefix_tail: Stamp::from_nonzero(std::num::NonZeroU64::MIN),
            table: None,
        }
    }

    /// Returns the permanent arena identity, racing to assign one (together
    /// with `current`) on the first call if several threads reach it
    /// concurrently. A losing thread's freshly minted (but unpublished)
    /// stamp is simply discarded — stamp values are never recycled, so this
    /// costs at most a few wasted values from the process-wide pool, never a
    /// correctness issue.
    #[inline]
    pub(crate) fn birth(&self) -> Stamp {
        std::num::NonZeroU64::new(self.birth.load(Ordering::Relaxed))
            .map_or_else(|| self.assign(), Stamp::from_nonzero)
    }

    /// Returns the stamp a new allocation receives right now, assigning one
    /// (together with `birth`) on the first call.
    #[inline]
    pub(crate) fn current(&self) -> Stamp {
        std::num::NonZeroU64::new(self.current.load(Ordering::Relaxed))
            .map_or_else(|| self.assign(), Stamp::from_nonzero)
    }

    /// Returns the raw current stamp (`0` when unassigned) without forcing
    /// assignment — the [`matches`](Self::matches) fast path only ever needs
    /// to compare it.
    #[inline]
    pub(crate) fn current_raw(&self) -> u64 {
        self.current.load(Ordering::Relaxed)
    }

    /// Returns the raw birth stamp (`0` when unassigned) without forcing
    /// assignment, for `validate_checkpoint`'s foreign-arena check.
    #[inline]
    pub(crate) fn birth_raw(&self) -> u64 {
        self.birth.load(Ordering::Relaxed)
    }

    #[cold]
    #[inline(never)]
    fn assign(&self) -> Stamp {
        let fresh = Stamp::fresh();
        if self
            .birth
            .compare_exchange(0, fresh.get(), Ordering::Relaxed, Ordering::Relaxed)
            .is_ok()
        {
            self.current.store(fresh.get(), Ordering::Relaxed);
        }
        // Re-read unconditionally: either this call just won the race and
        // `current` was just stored above, or another thread already won it
        // and is in the process of publishing `current` right now.
        loop {
            let raw = self.current.load(Ordering::Relaxed);
            if let Some(raw) = std::num::NonZeroU64::new(raw) {
                return Stamp::from_nonzero(raw);
            }
            std::hint::spin_loop();
        }
    }

    /// Returns the stamp that currently covers `slot`.
    #[inline]
    pub(crate) fn stamp_of(&self, slot: usize) -> Stamp {
        if slot >= self.current_start {
            self.current()
        } else if slot + 1 == self.current_start {
            // See `Identity::stamp_of`'s companion comment: `current_start >
            // 0` is implied here, so `prefix_tail` is guaranteed meaningful.
            self.prefix_tail
        } else {
            self.stamp_of_cold(slot)
        }
    }

    /// Reordered validation companion to [`Identity::matches`]: an index
    /// whose stamp equals the raw current value was minted after the last
    /// commit, hence at a slot `>= current_start`; the bounds/archive check
    /// is needed only for a stamp that differs from `current`. See the
    /// module documentation's "Reordered validation" section.
    #[inline]
    pub(crate) fn matches(&self, slot: usize, stamp: Stamp) -> bool {
        stamp.get() == self.current_raw() || self.matches_archived(slot, stamp)
    }

    #[cold]
    #[inline(never)]
    pub(crate) fn matches_archived(&self, slot: usize, stamp: Stamp) -> bool {
        slot < self.current_start && self.stamp_of_cold(slot) == stamp
    }

    #[cold]
    #[inline(never)]
    fn stamp_of_cold(&self, slot: usize) -> Stamp {
        let Some(table) = self.table.as_deref() else {
            return self.birth();
        };
        let covering = table.partition_point(|(start, _)| *start <= slot);
        covering
            .checked_sub(1)
            .map_or_else(|| self.birth(), |index| table[index].1)
    }

    /// Commits a new segment boundary. See [`Identity::commit`] for the
    /// ordering contract, the compaction rule, and the `prefix_tail`
    /// branches; all apply identically here.
    pub(crate) fn commit(&mut self, new_len: usize) {
        let birth_raw = self.birth_raw();
        if birth_raw == 0 {
            // Nothing has ever been issued by this arena: no index can be
            // stale, and no checkpoint of this arena's can exist yet
            // (`checkpoint` itself would have assigned an identity). Drawing
            // stamps here would only waste them.
            return;
        }
        let old_start = self.current_start;
        let current_raw = self.current_raw();
        let old_current = (current_raw != birth_raw).then(|| {
            Stamp::from_nonzero(
                std::num::NonZeroU64::new(current_raw)
                    .expect("a non-birth current stamp is never the unassigned sentinel"),
            )
        });
        if let Some(old_current) = old_current {
            let table = self.table.get_or_insert_with(|| Box::new(Vec::new()));
            let dead_from = table.partition_point(|(start, _)| *start < self.current_start);
            table.truncate(dead_from);
            table.push((self.current_start, old_current));
        }
        self.current.store(Stamp::fresh().get(), Ordering::Relaxed);
        self.current_start = new_len;

        // See `Identity::commit`'s matching `match` for the rationale behind
        // each branch below.
        self.prefix_tail = match old_current {
            _ if new_len == 0 => self.prefix_tail,
            Some(old_current) if new_len > old_start => old_current,
            Some(_) => self.stamp_of_cold(new_len - 1),
            None => self.birth(),
        };
    }
}
