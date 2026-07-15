//! Safe bump-pointer arena allocator.
//!
//! `safe-bump` provides a typed arena allocator built entirely with safe Rust
//! (zero `unsafe` blocks). Values are allocated and accessed through
//! unforgeable [`Idx<T>`] capabilities. Batch allocation returns a [`Block<T>`]
//! that is the only public way to derive indices within a contiguous batch.
//!
//! # Arena type
//!
//! [`Arena<T>`] stores values contiguously in a [`Vec<T>`]. Allocation stamps
//! live in a parallel metadata vector, so sequential value traversal retains
//! the locality of an ordinary vector. Checkpoints are bound to one arena and
//! one historical allocation prefix.
//!
//! # Key properties
//!
//! - **Zero `unsafe`**: enforced by `#![forbid(unsafe_code)]`
//! - **Auto [`Drop`]**: destructors run on reset, rollback, and arena drop
//! - **Unforgeable handles**: no public raw-index constructor
//! - **ABA resistance**: reused slots receive fresh allocation stamps
//! - **Validated rollback**: foreign and diverged checkpoints are rejected
//! - **Explicit blocks**: batch contiguity is carried by [`Block<T>`]
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
mod block;
mod checkpoint;
#[cfg(feature = "experimental-shared")]
mod chunked_storage;
mod idx;
mod iter;
#[cfg(feature = "experimental-shared")]
mod shared_arena;
mod stamp;

pub use arena::Arena;
pub use block::{Block, BlockIndices};
pub use checkpoint::{Checkpoint, CheckpointError};
pub use idx::Idx;
pub use iter::{ArenaDrain, ArenaIntoIter, IterIndexed, IterIndexedMut};
#[cfg(feature = "experimental-shared")]
pub use shared_arena::{SharedArena, SharedArenaIter, SharedArenaIterIndexed};

#[cfg(test)]
mod tests;
