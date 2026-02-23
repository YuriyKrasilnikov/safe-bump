# safe-bump

Safe bump-pointer arena allocator for Rust.

**Zero `unsafe`. Auto `Drop`. Checkpoint/rollback. Thread-safe.**

## Why safe-bump?

| Feature | `safe-bump` | `bumpalo` | `typed-arena` | `bump-scope` |
|---------|------------|-----------|---------------|-------------|
| `unsafe` code | **none** | yes | yes | yes |
| `#![forbid(unsafe_code)]` | **yes** | no | no | no |
| Auto `Drop` | **yes** | no | yes | yes |
| Checkpoint/rollback | **yes** | no | no | scopes only |
| Keep OR discard | **yes** | discard only | neither | discard only |
| Thread-safe arena | **`SharedArena`** | no | no | no |
| Access returns `&T` | **yes** | yes | yes | no (`BumpBox`) |

Existing arena allocators rely on `unsafe` internally for pointer manipulation.
`safe-bump` achieves the same arena semantics using only safe Rust.

## Two arena types

### `Arena<T>` — single-thread, zero overhead

Backed by `Vec<T>`. Minimal overhead, cache-friendly linear layout.

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
```

### `SharedArena<T>` — multi-thread, `Send + Sync`

Concurrent allocation via `&self`. Wait-free reads. Same `Idx<T>` handles,
same `&T` access, same checkpoint/rollback semantics.

```rust
use safe_bump::{SharedArena, Idx};
use std::sync::Arc;
use std::thread;

let arena = Arc::new(SharedArena::<u64>::new());

let handles: Vec<_> = (0..4).map(|i| {
    let arena = Arc::clone(&arena);
    thread::spawn(move || arena.alloc(i))
}).collect();

let indices: Vec<Idx<u64>> = handles.into_iter().map(|h| h.join().unwrap()).collect();

// All values accessible via &T — no guards, no locks
for idx in &indices {
    let _val: &u64 = arena.get(*idx);
}
```

### Comparison

```
                Arena<T>              SharedArena<T>
                ────────              ──────────────
alloc           &mut self, O(1)       &self, O(1)
get             &self → &T, O(1)     &self → &T, O(1) wait-free
rollback        &mut self             &mut self
memory/slot     sizeof(T)             sizeof(T) + ~8 bytes
cache           linear (Vec)          chunked (pointer chase)
threading       Send                  Send + Sync
unsafe          none                  none
```

### When to use which

**`Arena<T>`** — default choice for single-thread workloads:
- Backed by `Vec<T>`: one contiguous allocation, cache-friendly sequential access
- `alloc` is a single `Vec::push` — no atomic operations
- `get` is a direct array index — one memory access
- Supports `IndexMut`, `iter_mut`, `Extend`, `FromIterator`

**`SharedArena<T>`** — when multiple threads allocate concurrently:
- `alloc(&self)` can be called from any thread without `&mut`
- `get` returns `&T` directly (no `MutexGuard`, no `RwLockReadGuard`)
- Reads are wait-free: never blocked by concurrent writers

**The tradeoff:**

| | `Arena<T>` | `SharedArena<T>` |
|---|---|---|
| `get` latency | ~1 ns (direct index) | ~5-10 ns (two pointer chases) |
| `alloc` latency | ~5 ns (Vec::push) | ~10-20 ns (atomics + OnceLock) |
| Memory per slot | `size_of::<T>()` | `size_of::<T>()` + ~8 bytes |
| Empty arena | 0 bytes | ~512 bytes |
| Cache behavior | linear scan friendly | scattered across heap |
| Mutable access | `get_mut`, `IndexMut` | not available |

The overhead comes from the fundamental requirement: returning `&T` from
concurrent storage without locks requires indirection. Each slot is an
`OnceLock<T>` inside a chunked layout where elements never move — this
adds a pointer chase per access and ~8 bytes per slot for the `OnceLock`
bookkeeping.

If your code is single-threaded, always prefer `Arena<T>` — there is no
reason to pay for synchronization you don't use.

## Design

`Idx<T>` is a stable, `Copy` index valid for the lifetime of the arena
(invalidated by rollback/reset past its allocation point).

`Checkpoint<T>` captures allocation state. Rolling back drops all values
allocated after the checkpoint and reclaims their slots.

Both arena types share the same `Idx<T>` and `Checkpoint<T>` types.

### Complexity

| Operation | `Arena<T>` | `SharedArena<T>` |
|-----------|-----------|-----------------|
| `alloc` | O(1) amortized | O(1) |
| `get` / `Index` | O(1) | O(1) wait-free |
| `checkpoint` | O(1) | O(1) |
| `rollback` | O(k) | O(k) |
| `reset` | O(n) | O(n) |
| `alloc_extend` | O(n) | O(n) |
| `drain` | O(n) | O(n) |

k = items dropped (destructors run), n = all items.

### Standard traits

`Arena<T>`: `Index`, `IndexMut`, `IntoIterator`, `Extend`, `FromIterator`, `Default`.

`SharedArena<T>`: `Index`, `IntoIterator`, `Default`, `Send + Sync`.

`Idx<T>`: `Copy`, `Eq`, `Ord`, `Hash`, `Debug`.

`Checkpoint<T>`: `Copy`, `Eq`, `Ord`, `Hash`, `Debug`.

## Limitations

- **Typed**: each arena stores a single type `T`. Use separate arenas for
  different types.
- **Append-only**: individual items cannot be removed. Use `rollback` to
  discard a suffix or `reset` to clear everything.
- **`SharedArena` overhead**: ~8 bytes per slot and chunked storage layout
  (reduced cache locality) compared to `Arena`.
- **No cross-arena safety**: `Idx<T>` does not carry an arena identifier.
  An index from one arena can be used on another arena of the same type
  (panic on out-of-bounds, wrong data if in-bounds). This is a deliberate
  tradeoff: keeping `Idx` at one machine word minimizes storage overhead and
  eliminates per-access checks on the hot path.

## References

- Hanson, 1990 — "Fast Allocation and Deallocation of Memory Based on Object Lifetimes"

## License

Apache License 2.0. See [LICENSE](LICENSE).
