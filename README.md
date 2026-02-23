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
for rolled-back values. This pattern is essential for version-based reclamation
(VBR) workflows where mutations are validated before committing.

## Usage

```rust
use safe_bump::Arena;

let mut arena = Arena::new();
let a = arena.alloc(String::from("hello"));
let b = arena.alloc(String::from("world"));

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

Allocation is O(1) amortized (`Vec::push`). Access is O(1) (direct index).
Rollback is O(k) where k = items dropped. Reset is O(n).

## References

- Hanson, 1990 — "Fast Allocation and Deallocation of Memory Based on Object Lifetimes"

## License

Apache License 2.0. See [LICENSE](LICENSE).
