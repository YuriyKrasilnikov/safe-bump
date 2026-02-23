use crate::Idx;

/// Iterator yielding `(Idx<T>, &T)` pairs in allocation order.
///
/// Created by [`Arena::iter_indexed`](crate::Arena::iter_indexed).
pub struct IterIndexed<'a, T> {
    inner: std::iter::Enumerate<std::slice::Iter<'a, T>>,
}

impl<'a, T> IterIndexed<'a, T> {
    /// Creates a new indexed iterator from an enumerated slice iterator.
    #[must_use]
    pub const fn new(inner: std::iter::Enumerate<std::slice::Iter<'a, T>>) -> Self {
        Self { inner }
    }
}

impl<'a, T> Iterator for IterIndexed<'a, T> {
    type Item = (Idx<T>, &'a T);

    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next().map(|(i, v)| (Idx::from_raw(i), v))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

impl<T> ExactSizeIterator for IterIndexed<'_, T> {}

/// Mutable iterator yielding `(Idx<T>, &mut T)` pairs in allocation order.
///
/// Created by [`Arena::iter_indexed_mut`](crate::Arena::iter_indexed_mut).
pub struct IterIndexedMut<'a, T> {
    inner: std::iter::Enumerate<std::slice::IterMut<'a, T>>,
}

impl<'a, T> IterIndexedMut<'a, T> {
    /// Creates a new mutable indexed iterator from an enumerated slice
    /// iterator.
    #[must_use]
    pub const fn new(inner: std::iter::Enumerate<std::slice::IterMut<'a, T>>) -> Self {
        Self { inner }
    }
}

impl<'a, T> Iterator for IterIndexedMut<'a, T> {
    type Item = (Idx<T>, &'a mut T);

    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next().map(|(i, v)| (Idx::from_raw(i), v))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

impl<T> ExactSizeIterator for IterIndexedMut<'_, T> {}
