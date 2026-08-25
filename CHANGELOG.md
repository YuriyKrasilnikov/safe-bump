# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed

- Raise the minimum supported Rust version from 1.93 to 1.96.
- Record cross-version raw pairs under the `safe-bump-paired-raw-v2`
  schema with a closed paired workload matrix that separates allocation
  without growth, allocation with growth, arena creation and capacity
  reservation; name the execution order `baseline-candidate` and
  `candidate-baseline`, refuse zero-length intervals in the runner and the
  validator, and validate a two-repetition raw-pair file on pull requests.

## [0.3.0] - 2026-07-15

### Added

- Process-unique allocation stamps and arena identities.
- `Block<T>` as the bounded capability returned by contiguous batch
  allocation.
- `CheckpointError` and fail-closed `try_rollback`.
- Cross-arena, stale-ABA, diverged-prefix, destructor-unwind, and concurrent
  block checks.
- `experimental-shared` feature boundary for `SharedArena`.
- Public Criterion benchmarks for allocation, lookup, iteration, blocks,
  rollback, reset, and experimental concurrent allocation.
- A GitHub Actions benchmark workflow that compiles the harness on pull
  requests and publishes reports, raw output, and a runner passport for full
  runs.
- A v0.2.1-to-v0.3.0 cross-version Criterion harness, balanced AB/BA raw
  observations, and a diagnostic delta table, so the release exposes both
  performance differences and the measured cost of stronger capability
  validation on identical common-operation workloads without manufacturing a
  paired confidence interval.
- Amortized process-unique stamp allocation: globally disjoint ranges are
  reserved atomically per participating thread and shared by its arenas,
  without weakening foreign-arena or stale-ABA rejection.

### Changed

- `Arena<T>` keeps values contiguous while storing validation stamps in a
  parallel metadata vector.
- `Checkpoint<T>` now identifies an arena and a historical prefix rather than
  storing only a length.
- Concurrent batch allocation reserves its complete range atomically.
- `SharedArena` documentation now distinguishes wait-free published reads from
  potentially waiting allocation.

### Removed

- Public `Idx::from_raw`/`into_raw` and `Checkpoint::from_len` constructors.
- `alloc_extend`; callers use `alloc_block` and retain the block capability.
- `SharedArena` from the default feature set.

## [0.2.1] - 2026-02-23

### Added
- `SharedArena::iter` and `SharedArena::iter_indexed` — read-only iteration.
- `IntoIterator for &SharedArena<T>` — `for x in &arena` syntax.
- `Extend` and `FromIterator` trait impls for `SharedArena`.

### Changed
- README comparison table expanded to full API coverage.

## [0.2.0] - 2026-02-23

### Added
- `SharedArena<T>` — thread-safe (`Send + Sync`) arena with concurrent
  allocation via `&self` and wait-free reads returning `&T` directly.
- `ChunkedStorage<T>` — stable-address backing storage where occupied elements
  never move. Lazily allocated chunks with doubling sizes, zero `unsafe`, and
  synchronized initialization via `OnceLock`.
- Publication protocol (`reserved` + `published` atomics) ensuring
  contiguous-prefix visibility for readers.
- `SharedArena` methods: `alloc`, `get`, `try_get`, `is_valid`, `len`,
  `is_empty`, `checkpoint`, `rollback`, `reset`, `alloc_extend`, `drain`.
- `SharedArena` trait impls: `Index<Idx<T>>`, `IntoIterator`, `Default`.

### Changed
- `Idx::from_raw` and `Checkpoint::from_len` constructors became public.
- Crate-level documentation covers both arena types.

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
