# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.0] - 2026-02-23

### Added
- `SharedArena<T>` — thread-safe (`Send + Sync`) arena with concurrent
  allocation via `&self` and wait-free reads returning `&T` directly.
- `ChunkedStorage<T>` — append-only backing storage where elements never
  move after insertion. Lazily-allocated chunks with doubling sizes,
  zero `unsafe`, lock-free growth via `OnceLock`.
- Publication protocol (`reserved` + `published` atomics) ensuring
  contiguous-prefix visibility for readers.
- `SharedArena` methods: `alloc`, `get`, `try_get`, `is_valid`, `len`,
  `is_empty`, `checkpoint`, `rollback`, `reset`, `alloc_extend`, `drain`.
- `SharedArena` trait impls: `Index<Idx<T>>`, `IntoIterator`, `Default`.
- Concurrency stress tests: multi-thread allocation, barrier-synchronized
  writes, published consistency, livelock detection, contiguous-prefix
  verification, checkpoint during concurrent allocation.
- Property-based tests for checkpoint/rollback (proptest): random
  alloc/checkpoint/rollback sequences verified against a `Vec` model
  and a drop counter.

### Changed
- Crate split into modules per granularity rules: `arena.rs`, `idx.rs`,
  `checkpoint.rs`, `iter.rs`, `shared_arena.rs`, `chunked_storage.rs`.
- `lib.rs` reduced to module declarations and re-exports.
- Tests moved from inline `mod tests` to `src/tests/` directory with
  separate files for arena, shared arena, and property-based tests.
- `Idx::from_raw` and `Checkpoint::from_len` constructors made public
  to enable cross-module construction without `pub(crate)`.
- `Tracked` drop-counter helper defined once at module level instead of
  duplicated in four test functions.
- Crate-level documentation updated for two arena types.

## [0.1.0] - 2026-02-23

Initial release.

### Added
- `Arena<T>` — typed bump-pointer arena backed by `Vec<T>`.
- `Idx<T>` — stable, `Copy` index handle with `Eq`/`Ord`/`Hash`/`Debug`.
- `Checkpoint<T>` — saved allocation state with `Copy`/`Eq`/`Ord`.
- `alloc`, `alloc_extend` — single and batch allocation.
- `get`, `get_mut`, `try_get`, `try_get_mut`, `is_valid` — index access.
- `checkpoint`, `rollback`, `reset` — speculative allocation support.
- `iter`, `iter_mut`, `iter_indexed`, `iter_indexed_mut` — iteration.
- `drain`, `into_iter` — consuming iteration.
- `Index`/`IndexMut`, `Extend`, `FromIterator`, `IntoIterator` trait impls.
- `with_capacity`, `reserve`, `shrink_to_fit` — capacity management.
- `#![forbid(unsafe_code)]` — zero unsafe guarantee.
- `#![deny(missing_docs)]` — full documentation coverage.
