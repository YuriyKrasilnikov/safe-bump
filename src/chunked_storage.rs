use std::sync::OnceLock;

/// Append-only storage where elements never move after insertion.
///
/// Backed by a fixed array of lazily-allocated chunks with doubling sizes.
/// Chunk `k` has `2^k` slots. 32 chunks support up to `2^32 - 1` elements.
///
/// All operations are safe. Growth uses [`OnceLock::get_or_init`] (lock-free).
pub struct ChunkedStorage<T> {
    chunks: [OnceLock<Box<[OnceLock<T>]>>; 32],
}

impl<T> ChunkedStorage<T> {
    /// Creates an empty storage.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            chunks: [const { OnceLock::new() }; 32],
        }
    }

    /// Writes a value into the given slot. The chunk is allocated on demand.
    ///
    /// Returns `true` if the value was written, `false` if the slot was
    /// already occupied.
    pub fn set(&self, index: usize, value: T) -> bool {
        let (chunk_idx, offset) = split_index(index);
        let chunk = self.chunks[chunk_idx].get_or_init(|| alloc_chunk(chunk_idx));
        chunk[offset].set(value).is_ok()
    }

    /// Returns a reference to the value at `index`.
    ///
    /// # Panics
    ///
    /// Panics if the chunk is not allocated or the slot is empty.
    #[must_use]
    pub fn get(&self, index: usize) -> &T {
        let (chunk_idx, offset) = split_index(index);
        self.chunks[chunk_idx].get().expect("chunk not allocated")[offset]
            .get()
            .expect("slot is empty")
    }

    /// Returns `true` if the slot at `index` contains a value.
    ///
    /// Safe to call concurrently. Returns `false` if the chunk is not
    /// allocated or the slot is empty.
    #[must_use]
    pub fn is_set(&self, index: usize) -> bool {
        let (chunk_idx, offset) = split_index(index);
        self.chunks[chunk_idx]
            .get()
            .is_some_and(|chunk| chunk[offset].get().is_some())
    }

    /// Returns a reference to the value at `index`, or `None` if the slot
    /// is empty or the chunk is not allocated.
    #[cfg(test)]
    fn try_get(&self, index: usize) -> Option<&T> {
        let (chunk_idx, offset) = split_index(index);
        self.chunks[chunk_idx]
            .get()
            .and_then(|chunk| chunk[offset].get())
    }

    /// Takes the value out of the slot, leaving it empty.
    ///
    /// Requires `&mut self` — safe because exclusive access is guaranteed.
    pub fn take(&mut self, index: usize) -> Option<T> {
        let (chunk_idx, offset) = split_index(index);
        self.chunks[chunk_idx]
            .get_mut()
            .and_then(|chunk| chunk[offset].take())
    }
}

/// Splits a linear index into (chunk index, offset within chunk).
///
/// Chunk `k` has `2^k` slots and covers indices `[2^k - 1, 2^(k+1) - 2]`.
///
/// ```text
/// index 0 → chunk 0, offset 0   (chunk 0: 1 slot)
/// index 1 → chunk 1, offset 0   (chunk 1: 2 slots)
/// index 2 → chunk 1, offset 1
/// index 3 → chunk 2, offset 0   (chunk 2: 4 slots)
/// index 6 → chunk 2, offset 3
/// index 7 → chunk 3, offset 0   (chunk 3: 8 slots)
/// ```
const fn split_index(index: usize) -> (usize, usize) {
    let n = index + 1;
    let chunk = usize::BITS - n.leading_zeros() - 1;
    let offset = n - (1 << chunk);
    (chunk as usize, offset)
}

/// Allocates a chunk of `2^chunk_idx` empty `OnceLock` slots.
fn alloc_chunk<T>(chunk_idx: usize) -> Box<[OnceLock<T>]> {
    let size = 1_usize << chunk_idx;
    (0..size).map(|_| OnceLock::new()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_index_first_elements() {
        assert_eq!(split_index(0), (0, 0));
        assert_eq!(split_index(1), (1, 0));
        assert_eq!(split_index(2), (1, 1));
        assert_eq!(split_index(3), (2, 0));
        assert_eq!(split_index(4), (2, 1));
        assert_eq!(split_index(5), (2, 2));
        assert_eq!(split_index(6), (2, 3));
        assert_eq!(split_index(7), (3, 0));
    }

    #[test]
    fn set_and_get() {
        let storage = ChunkedStorage::new();
        assert!(storage.set(0, 100));
        assert!(storage.set(1, 200));
        assert!(storage.set(2, 300));

        assert_eq!(*storage.get(0), 100);
        assert_eq!(*storage.get(1), 200);
        assert_eq!(*storage.get(2), 300);
    }

    #[test]
    fn set_returns_false_on_duplicate() {
        let storage = ChunkedStorage::new();
        assert!(storage.set(0, 42));
        assert!(!storage.set(0, 99)); // already occupied
        assert_eq!(*storage.get(0), 42); // original value kept
    }

    #[test]
    fn try_get_empty() {
        let storage = ChunkedStorage::<i32>::new();
        assert_eq!(storage.try_get(0), None);
        assert_eq!(storage.try_get(999), None);
    }

    #[test]
    fn take_removes_value() {
        let mut storage = ChunkedStorage::new();
        storage.set(0, String::from("hello"));

        let taken = storage.take(0);
        assert_eq!(taken, Some(String::from("hello")));
        assert_eq!(storage.try_get(0), None);
    }

    #[test]
    fn take_empty_returns_none() {
        let mut storage = ChunkedStorage::<i32>::new();
        assert_eq!(storage.take(0), None);
    }

    #[test]
    fn slot_reuse_after_take() {
        let mut storage = ChunkedStorage::new();
        storage.set(0, 42);
        storage.take(0);

        // Slot is now empty, can be reused
        assert!(storage.set(0, 99));
        assert_eq!(*storage.get(0), 99);
    }

    #[test]
    fn many_elements() {
        let storage = ChunkedStorage::new();
        for i in 0..1000 {
            assert!(storage.set(i, i * 10));
        }
        for i in 0..1000 {
            assert_eq!(*storage.get(i), i * 10);
        }
    }

    #[test]
    fn drop_on_storage_drop() {
        use std::cell::Cell;
        use std::rc::Rc;

        struct D(Rc<Cell<u32>>);
        impl Drop for D {
            fn drop(&mut self) {
                self.0.set(self.0.get() + 1);
            }
        }

        let drop_count = Rc::new(Cell::new(0u32));
        {
            let storage = ChunkedStorage::new();
            storage.set(0, D(Rc::clone(&drop_count)));
            storage.set(1, D(Rc::clone(&drop_count)));
            storage.set(2, D(Rc::clone(&drop_count)));
            assert_eq!(drop_count.get(), 0);
        }
        assert_eq!(drop_count.get(), 3);
    }
}
