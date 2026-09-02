# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.3.0] - 2026-09-02

### Added

- Process-unique allocation stamps and arena identities.
- `Block<T>` as the bounded capability returned by contiguous batch
  allocation.
- `Arena::alloc_block` collects its input before mutating the arena, so an
  input-iterator panic leaves the whole arena at its original prefix.
  `SharedArena::alloc_block` collects its input the same way, but its
  `&self` receiver means that guarantee covers only its own reservation and
  publication: a reentrant iterator can allocate through the same arena
  before panicking, and that allocation stays published — unlike
  `Arena::alloc_block`, whose exclusive `&mut self` receiver rules out a
  reentrant caller-side mutation.
- `CheckpointError` and fail-closed `try_rollback`.
- Cross-arena, stale-ABA, diverged-prefix, destructor-unwind, and concurrent
  block checks.
- `experimental-shared` feature boundary for `SharedArena`.
- Public Criterion benchmarks for allocation, lookup, iteration, blocks,
  rollback, reset, and experimental concurrent allocation.
- A GitHub Actions benchmark workflow that compiles the harness on pull
  requests and publishes reports, raw output, and a runner passport for full
  runs.
- A v0.2.1-to-v0.3.0 cross-version Criterion harness, alternating AB/BA raw
  observations, and a diagnostic delta table, so the release exposes both
  performance differences and the measured cost of stronger capability
  validation on identical common-operation workloads without manufacturing a
  paired confidence interval.
- Amortized process-unique stamp allocation: globally disjoint ranges are
  reserved atomically per participating thread and shared by its arenas,
  without weakening foreign-arena or stale-ABA rejection.
- `Arena::validate_checkpoint` and `SharedArena::validate_checkpoint`: check
  a checkpoint against the current arena state (foreign owner, length, and
  diverged-prefix tail stamp) without mutating the arena. A caller holding
  several arenas can validate every checkpoint it intends to roll back
  before mutating any of them, turning an otherwise independent multi-arena
  rollback into an all-or-nothing operation with respect to
  `CheckpointError` — a panicking destructor in a discarded suffix can still
  interrupt an individual rollback.

### Changed

- `Arena::new` and `SharedArena::new` are no longer `const fn`. Both were
  `pub const fn` in 0.2.1; constructing an arena now assigns it a
  process-unique identity (an allocation stamp), which cannot be computed in
  a `const` context.
- `Arena<T>` keeps values contiguous while storing validation stamps in a
  parallel metadata vector.
- `Checkpoint<T>` now identifies an arena and a historical prefix rather than
  storing only a length.
- Concurrent batch allocation reserves its complete range atomically.
- `SharedArena` documentation now distinguishes wait-free published reads from
  potentially waiting allocation.
- Raise the minimum supported Rust version from 1.93 to 1.96.
- Record cross-version raw pairs under the `safe-bump-paired-raw-v2` schema
  with a closed paired workload matrix that separates allocation without
  growth, allocation with growth, arena creation and capacity reservation;
  alternate the execution order between `baseline-candidate` and
  `candidate-baseline` per repetition (an odd repetition count leaves one
  more `baseline-candidate` pair than `candidate-baseline`), refuse
  zero-length intervals in the runner and the validator, and validate a
  two-repetition raw-pair file on pull requests.
- `SharedArena::drain` now unpublishes and takes one slot at a time, from
  the last slot down to the first, decrementing `published`/`reserved`
  before each take, and panics if a currently published slot turns out to
  be empty. 0.2.1 instead ran a single forward pass and reset both counters
  once at the end, silently skipping any slot that violated that same
  published-slot invariant rather than surfacing it. 0.3.0 makes the
  invariant explicit, and because the counters are now updated per slot
  instead of once at the end, they describe exactly the untaken prefix at
  every step, so even that new panic path leaves the arena immediately
  consistent and reusable.

### Removed

- Public `Idx::from_raw`/`into_raw` and `Checkpoint::from_len` constructors.
- `alloc_extend`; callers use `alloc_block` and retain the block capability.
- `SharedArena` from the default feature set.

### Fixed

- `SharedArena::rollback` and `SharedArena::reset`: 0.2.1 updated
  `published`/`reserved` only after running every discarded value's
  destructor, so a destructor panic left the counters describing the
  pre-operation length over a slot whose value had already been dropped —
  the stale index still reported `is_valid`, while `try_get` and `iter`
  then panicked on the empty slot. 0.3.0 decrements a slot's counters
  before running that slot's destructor, so the same destructor panic
  leaves the arena immediately consistent: `len()` reports the correctly
  reduced count, `try_get` returns `None`, `iter` succeeds, and the arena
  stays usable for a retry.

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
