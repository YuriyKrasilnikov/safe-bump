# safe-bump

Safe typed arena with unforgeable indices, explicit contiguous blocks, and
validated checkpoint rollback.

[![Benchmarks](https://github.com/YuriyKrasilnikov/safe-bump/actions/workflows/benchmarks.yml/badge.svg?branch=main)](https://github.com/YuriyKrasilnikov/safe-bump/actions/workflows/benchmarks.yml)

`safe-bump` contains no `unsafe` code and enforces that property with
`#![forbid(unsafe_code)]`.

The minimum supported Rust version is 1.96.

## What 0.3.0 changes for a 0.2.1 user

In 0.2.1 an index was a plain vector offset with a public constructor, and a
checkpoint was a length. That is enough to read a value the handle was never
issued for, without any error:

```rust,ignore
// safe-bump 0.2.1
let mut arena = Arena::new();
let old = arena.alloc(String::from("old"));
arena.reset();
arena.alloc(String::from("current"));

assert_eq!(arena[old], "current");               // stale handle, no error
assert_eq!(arena[Idx::from_raw(0)], "current");  // handle built from a number
```

and to empty one arena with a checkpoint that belongs to another:

```rust,ignore
// safe-bump 0.2.1
let checkpoint = other.checkpoint();
arena.rollback(checkpoint);                      // accepted; arena is emptied
```

0.3.0 answers `None` for the stale handle, `Err(CheckpointError::ForeignArena)`
for the foreign checkpoint and `Err(CheckpointError::DivergedPrefix)` for a
prefix that was discarded and rebuilt, and `Idx::from_raw` no longer exists,
so a handle cannot be derived from a number at all. It also adds contiguous
block capabilities, `drain`, and a `Send + Sync` experimental shared arena.

These checks are the reason to upgrade and they are not free: see
[Layout and cost](#layout-and-cost) for the sizes and [Benchmarks](#benchmarks)
for the measured comparison against 0.2.1.

## Why the handles are capabilities

A raw vector offset is not enough to identify an arena value. The same offset
can exist in another arena, and rollback/reset can reuse it for a different
value. `Idx<T>` therefore carries:

- the absolute slot used for O(1) access;
- the process-unique stamp of the *generation segment* active when the slot
  was written — every allocation since the arena's last rollback, reset, or
  drain shares one such stamp.

There is no public constructor from a number. `Idx::slot()` is diagnostic only
and cannot be converted back into a handle. Access succeeds exactly when the
slot is live and its segment's stamp still matches the one the index carries.

```rust
use safe_bump::Arena;

let mut arena = Arena::new();
let old = arena.alloc(String::from("old"));
arena.reset();
let current = arena.alloc(String::from("current"));

assert_eq!(old.slot(), current.slot());
assert!(arena.try_get(old).is_none());
assert_eq!(arena.get(current), "current");
```

This rejects both cross-arena aliasing and the stale-index ABA case.

## Contiguous batch allocation

`alloc_block` returns a `Block<T>` capability rather than only the first
integer offset. The block proves one stamped half-open interval, and its
`get(offset)` method is the only public way to derive an index within it.

```rust
use safe_bump::Arena;

let mut arena = Arena::new();
let block = arena.alloc_block([10, 20, 30]);

assert_eq!(block.len(), 3);
assert_eq!(arena[block.get(1).unwrap()], 20);
assert!(block.get(3).is_none());
```

The iterator is collected before `Arena` state changes. If it panics, the
arena as a whole retains its original prefix — `alloc_block` borrows `Arena`
exclusively, so no other code can have mutated it in the meantime.

## Historical checkpoints

A checkpoint contains the arena's permanent birth identity, prefix length,
and the stamp of the segment that owned the prefix's tail slot at capture
time. `try_rollback` rejects:

- a checkpoint created by another arena;
- a checkpoint beyond the current length;
- an equal-length checkpoint whose old prefix was discarded and replaced.

```rust
use safe_bump::Arena;

let mut arena = Arena::new();
let root = arena.alloc("root");
let checkpoint = arena.checkpoint();
let stale = arena.alloc("temporary");

arena.rollback(checkpoint);
assert_eq!(arena[root], "root");
assert!(!arena.is_valid(stale));
```

Rollback and reset commit a fresh generation segment for the discarded range
*before* dropping any value in it, so every index into that range is already
rejected from the moment the call begins — regardless of how far the drop
loop has physically gotten when a destructor panics. If user `Drop` code
panics, the arena stays structurally consistent and the same rollback or
reset can be retried to finish dropping the rest of the range.

## Layout and cost

`Arena<T>` stores values in a contiguous `Vec<T>`, with no parallel per-slot
metadata vector. Capability validation reads one small, lazily assigned
identity instead: a permanent birth stamp, the stamp of the current
allocation segment, and a table of archived segments for slots an earlier
rollback, reset, or drain left behind. Validating a handle compares its
stamp against an inline mirror of the current segment stamp — one field
read, no matter how many values the arena holds — and only falls back to a
binary search over the archive table for a stamp that does not match; that
table stays empty until an arena has been invalidated more than once. The
identity itself lives behind a lazily assigned sidecar handle that costs a
fixed handful of bytes on `Arena<T>` whether or not it has ever been
assigned, and is only actually allocated on the first capability the arena
issues, not on construction. Each
participating thread reserves globally disjoint stamp ranges and dispenses
them locally, so ordinary allocation does not perform a process-global atomic
transition for every value. Ranges are shared by every arena used on that
thread, and reserved values are never recycled, preserving process-wide
uniqueness after arenas or threads are dropped.

| Operation | Complexity |
|---|---|
| `alloc` | O(1) amortized |
| `alloc_block(n)` | O(n) |
| `get`, `try_get`, indexing, `is_valid` | O(1) for a handle of the current segment, O(log s) for one that predates it |
| `checkpoint` | O(1) |
| `rollback(k)` | O(k), including destructors |
| `reset(n)`, `drain(n)` | O(n) |

Here `s` is the number of archived segments the arena still distinguishes.
An arena that never rolls back, resets or drains keeps `s` at zero and
answers every validation with one stamp comparison. A speculative loop that
rolls back to the same length keeps `s` at one, because committing a segment
drops the archived entries that start at or after the new boundary. `s` grows
only when an arena rolls back to a strictly increasing length again and
again, as a backtracking parser does; validating a handle issued before those
rollbacks then costs a binary search over the archive; the one slot
directly below the current segment, which is what `checkpoint` reads, is
cached and does not search.

## Experimental concurrent arena

`SharedArena<T>` is available only with the `experimental-shared` feature:

```toml
safe-bump = { version = "0.3.0", features = ["experimental-shared"] }
```

It reserves a whole batch with one atomic transition, so concurrent batches
cannot interleave their slots. Reads of already-published handles are
wait-free and return `&T` without a guard. Allocation cooperatively advances a
contiguous publication prefix and may busy-wait for an earlier reserving
thread; allocation is **not** claimed to be wait-free or starvation-free.
`alloc_block` takes `&self`, so its collected-before-committing guarantee on
input panic covers only its own reservation and publication: a reentrant
iterator holding another reference to the same arena can allocate and
publish through it before panicking, and that allocation stays published.

This feature remains experimental because allocation can wait and its API may
change before the feature is stabilized. The default crate exposes only
`Arena<T>`.

## Benchmarks

The repository contains Criterion benchmarks for the public operations whose
cost matters to arena users:

- allocation, validated lookup, sequential iteration, and reset, with `Vec`
  as the transparent storage baseline;
- individual allocation versus `alloc_block` over batch sizes from one to
  65,536 values;
- checkpoint creation, successful suffix rollback, and rejected foreign
  checkpoints;
- experimental `SharedArena` single-item and block allocation with one, two,
  four, and eight producer threads, plus published reads.

The `Vec` allocation branch materializes the raw slot corresponding to every
push, and the lookup branch reads pre-recorded raw slots just as the arena
branch reads pre-recorded capabilities. This keeps handle production and
consumption inside the same measurement boundary while leaving `Vec` as the
unvalidated storage floor.

Run the default arena benchmarks with:

```console
cargo bench --locked
```

Include the experimental concurrent arena with:

```console
cargo bench --locked --all-features
```

Compare the published v0.2.1 implementation with the current v0.3 source on
the same host and identical common-operation workloads with:

```console
cargo bench --locked --manifest-path benchmarks/release-comparison/Cargo.toml
```

The release comparison covers allocation, validated lookup, sequential
iteration, speculative rollback, and concurrent allocation. It deliberately
does not pretend that v0.2 has equivalents for v0.3 block capabilities. On the
project's CI runner, measured in one run with two instruments — the paired
alternating-order raw protocol and same-process Criterion medians — iteration
and shared allocation stay at v0.2.1 levels (0.99x to 1.13x), and allocation
costs about 1.2x to 1.4x. Validated lookup costs 1.10x to 1.21x by the
Criterion estimate and 1.6x to 2.3x by the paired estimate on that same run:
it pays for one stamp comparison against an inline mirror of the current
segment stamp, which is what makes a stale handle answer `None` instead of a
value. Speculative rollback costs about 1.2x to 1.4x for suffixes of 64 values
and more, and for a suffix of one value it moves from about 2.5 ns to about
29 ns, because 0.2.1 truncated a vector while 0.3.0 validates the checkpoint
and opens a new generation segment. The identity itself (birth stamp, current
stamp, current-start, archive table) is not allocated at all until an arena's
first capability — but the lazily-assigned sidecar handle plus the inline
mirror still cost a fixed handful of bytes on every arena, allocated or not,
so an empty arena is larger than in 0.2.1 — `size_of::<Arena<u64>>()` is 48
bytes versus 0.2.1's 24, and `size_of::<SharedArena<u64>>()` is 1592 bytes
versus 0.2.1's 784 — and creating many empty arenas costs about 1.8x, with the
gap widening with the number of arenas created rather than staying within a
fixed ratio. A slower validated lookup or arena creation is not
silently classified as a product regression: the report keeps each next to the
stronger arena-identity and allocation-history checks that v0.2 did not
perform. These are diagnostic comparisons on one host, not portable latency
guarantees.

Executable cross-version quality witnesses demonstrate the corresponding
foreign-index, stale-ABA, and foreign-checkpoint failures in v0.2.1 and their
rejection in v0.3.0. See
[`benchmarks/release-comparison/QUALITY.md`](https://github.com/YuriyKrasilnikov/safe-bump/blob/main/benchmarks/release-comparison/QUALITY.md).

Criterion writes its local report under `target/criterion`. The GitHub
`Benchmarks` workflow compiles every benchmark on pull requests and performs a
full run on `main`, version tags, a weekly schedule, and manual dispatch. Each
full run publishes current and cross-version Criterion reports, a diagnostic
point-ratio table carrying each function's separate marginal confidence
interval, alternating AB/BA raw observations with content witnesses, console
output, and a runner passport. The workflow deliberately does not claim a
paired confidence interval or a performance winner from one hosted run.

Wall-clock results depend on the compiler, CPU, operating system, and runner
load. Compare results from the same host class and inspect the attached runner
passport; a GitHub-hosted timing is evidence for that run, not a portable
latency guarantee.

## Public API summary

- `Arena<T>`: `alloc`, `alloc_block`, stamped access, iteration, drain,
  checkpoint/rollback/reset, capacity control.
- `Idx<T>`: `Copy`, `Eq`, `Ord`, `Hash`, `Debug`; no raw constructor.
- `Block<T>`: bounded index derivation and exact-size index iteration.
- `Checkpoint<T>`: arena-bound historical prefix.
- `CheckpointError`: fail-closed rollback diagnostics.

## Current platform boundary

The stamp allocator requires native 64-bit atomics. Targets without
`target_has_atomic = "64"` fail at compile time instead of silently weakening
identity or allowing wraparound. Stamp exhaustion also fails before reuse.

## License

Apache License 2.0. See [LICENSE](https://github.com/YuriyKrasilnikov/safe-bump/blob/main/LICENSE).
