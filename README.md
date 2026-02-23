# safe-bump

Safe bump-pointer arena allocator for Rust.

**Zero `unsafe`. Auto `Drop`. Checkpoint/rollback.**

## Why safe-bump?

| Feature | `safe-bump` | `bumpalo` | `typed-arena` | `bump-scope` |
|---------|------------|-----------|---------------|-------------|
| `unsafe` code | **none** | yes | yes | yes |
| `#![forbid(unsafe_code)]` | **yes** | no | no | no |
| Auto `Drop` | **yes** | no | yes | yes |
| Checkpoint/rollback | **yes** | no | no | scopes only |
| Reset with reuse | **yes** | yes | no | yes |
| Access pattern | index `Idx<T>` | reference `&T` | reference `&T` | `BumpBox<T>` |
| Keep OR discard | **yes** | discard only | neither | discard only |

Existing arena allocators (`bumpalo`, `typed-arena`, `bump-scope`) all rely on
`unsafe` internally for pointer manipulation. `safe-bump` achieves the same
arena semantics using only safe Rust: values are stored in a `Vec<T>` and
accessed via typed `Idx<T>` handles.

The index-based design enables **checkpoint/rollback** — save allocation state,
allocate speculatively, then either keep or discard. Discarding runs destructors
for rolled-back values. This pattern is useful when mutations must be validated
before committing — allocate tentatively, check invariants, then keep or roll back.

## Usage

```rust
use safe_bump::{Arena, Idx};

let mut arena: Arena<String> = Arena::new();
let a: Idx<String> = arena.alloc(String::from("hello"));
let b: Idx<String> = arena.alloc(String::from("world"));

assert_eq!(arena[a], "hello");
assert_eq!(arena[b], "world");

// Checkpoint and rollback
let cp = arena.checkpoint();
let _tmp = arena.alloc(String::from("temporary"));
assert_eq!(arena.len(), 3);

arena.rollback(cp); // "temporary" is dropped
assert_eq!(arena.len(), 2);

// Reset reuses memory
arena.reset();
assert_eq!(arena.len(), 0);
```

## Design

`Arena<T>` is a typed, append-only allocator backed by `Vec<T>`.
`Idx<T>` is a stable, `Copy` index into the arena.
`Checkpoint<T>` captures allocation state for rollback.

### Complexity

| Operation | Time | Notes |
|-----------|------|-------|
| `alloc` | O(1) amortized | `Vec::push` |
| `get` / `Index` | O(1) | direct index |
| `alloc_extend` | O(n) | n = items from iterator |
| `checkpoint` | O(1) | saves current length |
| `rollback` | O(k) | k = items dropped (destructors run) |
| `reset` | O(n) | n = all items (destructors run) |
| `drain` | O(n) | returns owning iterator |
| `reserve` | O(1) amortized | delegates to `Vec::reserve` |

### Standard traits

`Arena<T>` implements `Index<Idx<T>>`, `IndexMut<Idx<T>>`, `IntoIterator`
(shared, mutable, and consuming), `Extend<T>`, `FromIterator<T>`, and `Default`.

`Idx<T>` implements `Copy`, `Eq`, `Ord`, `Hash`, and `Debug`.

`Checkpoint<T>` implements `Copy`, `Eq`, `Ord`, `Hash`, and `Debug`.

## Limitations

- **Typed**: each `Arena<T>` stores a single type. Use separate arenas for
  different types.
- **Append-only**: individual items cannot be removed. Use `rollback` to
  discard a suffix or `reset` to clear everything.
- **No cross-arena safety**: `Idx<T>` is a plain `usize` wrapper — it does
  not carry an arena identifier. An index from one arena can be accidentally
  used on another arena of the same type (panic on out-of-bounds, silent
  wrong data if in-bounds). This is a deliberate tradeoff: keeping `Idx` at
  one machine word (8 bytes, `Copy`) minimizes storage overhead in data
  structures that hold many indices, and eliminates per-access arena-id
  checks on the hot path. The same approach is used by `typed-arena` (bare
  references), `slotmap` (keys without container id), and ECS libraries.

## References

- Hanson, 1990 — "Fast Allocation and Deallocation of Memory Based on Object Lifetimes"

## License

Apache License 2.0. See [LICENSE](LICENSE).
