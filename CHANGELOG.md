# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- Property-based tests for checkpoint/rollback (proptest): random
  alloc/checkpoint/rollback sequences verified against a `Vec` model
  and a drop counter.

### Changed
- Tests moved from inline `mod tests` to `src/tests.rs` submodule
  (production code and tests separated per module granularity rules).
- `Tracked` drop-counter helper defined once at module level instead of
  duplicated in four test functions.
- `rollback_beyond_length_panics` rewritten to use public API only
  (no private field access).

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
