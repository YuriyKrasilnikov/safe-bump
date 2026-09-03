use super::*;

/// `Idx<T>` is a stamp and a slot: `NonZeroU64` plus `usize`, 16 bytes on a
/// 64-bit target, with no padding. `Stamp`'s `NonZeroU64` niche lets
/// `Option<Idx<T>>` reuse the all-zero bit pattern for `None`, so it costs
/// nothing beyond `Idx<T>` itself.
#[test]
fn idx_is_sixteen_bytes_and_niche_optimized() {
    assert_eq!(std::mem::size_of::<Idx<u64>>(), 16);
    assert_eq!(std::mem::size_of::<Option<Idx<u64>>>(), 16);
}

/// `Checkpoint<T>` is an owner stamp, a length, and an optional tail stamp:
/// `NonZeroU64` + `usize` + `Option<NonZeroU64>`, 24 bytes on a 64-bit
/// target once the tail's niche folds into the same word class as the
/// owner field.
#[test]
fn checkpoint_is_twenty_four_bytes() {
    assert_eq!(std::mem::size_of::<Checkpoint<u64>>(), 24);
}

/// `Arena<T>`'s identity lives behind an `OnceLock<Box<Identity>>` sidecar
/// plus one inline `AtomicU64` mirror, not a `Cell`-based inline layout: a
/// consumer embedding `Arena` in a type that asserts `Send + Sync` (for
/// example a newtype like champ-trie's `ChampMapSync`) relies on `Arena<T>`
/// itself staying `Sync` for `Sync` `T`. This is a compile-time check: the
/// test body runs trivially, but it would fail to *compile* if `Arena<T>`
/// or `SharedArena<T>` ever lost `Sync`.
#[test]
fn arena_is_sync_when_values_are() {
    fn assert_sync<T: Sync>() {}
    assert_sync::<Arena<u64>>();
}

#[cfg(feature = "experimental-shared")]
#[test]
fn shared_arena_is_sync_when_values_are() {
    fn assert_sync<T: Sync>() {}
    assert_sync::<SharedArena<u64>>();
}

/// `Arena<T>` is a `Vec<T>` (24 bytes) plus an `OnceLock<Box<Identity>>`
/// sidecar (16 bytes: the `Once` completion state plus the boxed pointer,
/// no niche shared between them) plus the inline `AtomicU64` mirror (8
/// bytes) — 48 bytes total, independent of how many values the arena holds.
#[test]
fn arena_is_forty_eight_bytes() {
    assert_eq!(std::mem::size_of::<Arena<u64>>(), 48);
}
