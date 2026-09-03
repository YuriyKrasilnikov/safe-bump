# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Fixed

- A checkpoint taken with no allocation since the last `rollback`, `reset`
  or `drain` read the slot below the current segment through a binary
  search over the archived segments, so its cost grew with the number of
  those segments. The identity now caches that one stamp, and the search
  is left to handles that reach further back.

### Changed

- `size_of::<SharedArena<u64>>()` is 1592 bytes, up from 1584, because the
  identity `SharedArena` embeds carries the cached stamp.
  `size_of::<Arena<u64>>()` is unchanged at 48 bytes: its identity lives
  behind a pointer.

## [0.3.0] - 2026-09-03

This release is about what the arena refuses to do. In 0.2.1 a stale `Idx`
read whatever value later occupied its slot, an `Idx` could be built from any
number through `from_raw`, and `rollback` accepted any checkpoint short enough
to fit, including one belonging to a different arena. 0.3.0 rejects all three,
adds contiguous block capabilities and `drain`, and makes the experimental
shared arena `Send + Sync`. The checks are paid for on validated reads, on
arena creation and on short speculative rollbacks, and in the size of an empty
arena; the measured picture is under "Changed".

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
- `Arena::validate_checkpoint` and `SharedArena::validate_checkpoint`: check
  a checkpoint against the current arena state (foreign owner, length, and
  diverged-prefix tail stamp) without mutating the arena. A caller holding
  several arenas can validate every checkpoint it intends to roll back
  before mutating any of them, turning an otherwise independent multi-arena
  rollback into an all-or-nothing operation with respect to
  `CheckpointError` — a panicking destructor in a discarded suffix can still
  interrupt an individual rollback.
- Cross-arena, stale-ABA, diverged-prefix, destructor-unwind, and concurrent
  block checks.
- `experimental-shared` feature boundary for `SharedArena`.
- Public Criterion benchmarks for allocation, lookup, iteration, blocks,
  rollback, reset, and experimental concurrent allocation.
- A GitHub Actions benchmark workflow that compiles the harness on pull
  requests and publishes reports, raw output, and a runner passport for full
  runs.
- A v0.2.1-to-v0.3.0 cross-version Criterion harness, executable quality
  witnesses that reproduce v0.2.1's rejected-capability gaps and confirm
  their rejection here, alternating AB/BA raw observations, and a
  diagnostic delta table, so the release exposes both performance
  differences and the measured cost of stronger capability validation on
  identical common-operation workloads without manufacturing a paired
  confidence interval.
- A layout guarantee test pinning `size_of::<Idx<u64>>()` and
  `size_of::<Option<Idx<u64>>>()` at 16 bytes and `size_of::<Checkpoint<u64>>()`
  at 24 bytes.

### Changed

- **Capability model**: an arena no longer draws a fresh stamp for every
  `alloc`/`alloc_block` call and stores it in a parallel per-slot vector.
  Instead, each arena keeps one lazily assigned identity — a permanent birth
  stamp, the stamp of the current *generation segment* (every allocation
  since the last invalidating operation shares it), and a small table of
  archived segments for slots an earlier `rollback`/`reset`/`drain` left
  behind. A `rollback`, `reset`, or `drain` draws a fresh process-unique
  stamp for the new segment and commits the boundary *before* dropping (or,
  for `SharedArena`, taking) any value in the invalidated range, so a
  destructor panic can never leave a not-yet-removed slot reporting a stamp
  a stale `Idx` still carries. Validating a handle is one field comparison
  in the common case instead of a parallel-vector index; a slot that
  predates the current segment falls back to a binary search over the
  (typically empty or tiny) archive table. The identity itself is assigned
  lazily, on the first capability an arena issues (`alloc`, `alloc_block`,
  or `checkpoint`), not in `new`/`with_capacity`.
- `Arena::new` and `SharedArena::new` are `const fn` again: since identity
  assignment is lazy, constructing an arena no longer needs a stamp, and a
  `const` empty arena has no identity until its first capability.
- `Checkpoint<T>` identifies an arena and a historical prefix rather than
  storing only a length.
- `Arena::capacity` reports the backing value vector's capacity directly;
  there is no longer a parallel metadata vector for it to reconcile against.
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
- Performance, measured against v0.2.1 on the project's CI runner with two
  instruments in one run: the paired alternating-order raw protocol and
  same-process Criterion medians. Iteration and shared allocation stay at
  v0.2.1 levels (0.99x to 1.13x). Allocation costs about 1.2x to 1.4x.
  Validated lookup costs 1.10x to 1.21x by the Criterion estimate and 1.6x to
  2.3x by the paired estimate on that same run; it buys the check that a
  handle still names the value it was issued for. Speculative rollback costs
  about 1.2x to 1.4x for suffixes of 64 values and more, and for a suffix of
  one value it moves from about 2.5 ns to about 29 ns, because v0.2.1
  truncated a vector while v0.3.0 validates the checkpoint and opens a new
  generation segment. `Arena` keeps its identity (birth stamp, current stamp,
  current-start, archive table) behind one lazily assigned
  `OnceLock<Box<Identity>>` sidecar, so it is not allocated at all until the
  arena's first capability — but the sidecar handle plus the inline mirror
  still cost a fixed handful of bytes on every `Arena`, allocated or not, so
  an empty arena is larger than in 0.2.1 — `size_of::<Arena<u64>>()` is 48
  bytes versus 0.2.1's 24, and `size_of::<SharedArena<u64>>()` is 1584 bytes
  versus 0.2.1's 784. Creating many empty arenas costs about 1.8x, and the gap
  widens with the number of arenas created rather than staying within a fixed
  ratio. These are diagnostic comparisons on one host, not portable latency
  guarantees — see `benchmarks/release-comparison/QUALITY.md`.
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
- A raw v0.2.1 slot index can accidentally read an equally numbered slot in
  a foreign arena; 0.3.0 rejects the capability instead.
- A v0.2.1 stale index left over after rollback and slot reuse retargets the
  replacement value (the ABA problem); 0.3.0 rejects it while accepting the
  fresh index.
- A v0.2.1 checkpoint is only a length and can be applied to another arena;
  0.3.0 returns `CheckpointError::ForeignArena` without changing the target.
- A v0.2.1 `SharedArena::alloc_extend` reserves one slot per yielded item
  through repeated `&self` calls to `alloc`, so a concurrent or reentrant
  allocation between two yields can land inside the slot range the batch
  occupies; 0.3.0's `SharedArena::alloc_block` reserves its whole range in
  one atomic step, so the returned `Block` cannot be interleaved.

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
