//! Safe bump-pointer arena allocator.
//!
//! `safe-bump` provides two typed arena allocators built entirely with safe
//! Rust (zero `unsafe` blocks). Values are allocated and accessed via stable
//! [`Idx<T>`] indices.
//!
//! # Arena types
//!
//! - [`Arena<T>`] — single-thread, zero overhead, backed by [`Vec<T>`]
//! - [`SharedArena<T>`] — thread-safe (`Send + Sync`), wait-free reads,
//!   concurrent allocation via `&self`
//!
//! Both types share the same [`Idx<T>`] and [`Checkpoint<T>`] types, support
//! checkpoint/rollback, and run destructors on rollback/reset/drop.
//!
//! # Key properties
//!
//! - **Zero `unsafe`**: enforced by `#![forbid(unsafe_code)]`
//! - **Auto [`Drop`]**: destructors run on reset, rollback, and arena drop
//! - **Checkpoint/rollback**: save state and discard speculative allocations
//! - **Thread-safe**: [`SharedArena<T>`] supports concurrent allocation
//!
//! # Example
//!
//! ```
//! use safe_bump::{Arena, Idx};
//!
//! let mut arena: Arena<String> = Arena::new();
//! let a: Idx<String> = arena.alloc(String::from("hello"));
//! let b: Idx<String> = arena.alloc(String::from("world"));
//!
//! assert_eq!(arena[a], "hello");
//! assert_eq!(arena[b], "world");
//!
//! let cp = arena.checkpoint();
//! let _tmp = arena.alloc(String::from("temporary"));
//! arena.rollback(cp); // "temporary" is dropped
//! assert_eq!(arena.len(), 2);
//! ```
//!
//! # References
//!
//! - Hanson, 1990 — "Fast Allocation and Deallocation of Memory
//!   Based on Object Lifetimes"

#![forbid(unsafe_code)]
#![deny(missing_docs)]

mod arena;
mod checkpoint;
mod chunked_storage;
mod idx;
mod iter;
mod shared_arena;

pub use arena::Arena;
pub use checkpoint::Checkpoint;
pub use idx::Idx;
pub use iter::{IterIndexed, IterIndexedMut};
pub use shared_arena::{SharedArena, SharedArenaIter, SharedArenaIterIndexed};

#[cfg(test)]
mod tests;
