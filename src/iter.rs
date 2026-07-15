use crate::Idx;
use crate::stamp::Stamp;

/// Iterator yielding `(Idx<T>, &T)` pairs in allocation order.
pub struct IterIndexed<'a, T> {
    stamps: std::slice::Iter<'a, Stamp>,
    values: std::slice::Iter<'a, T>,
    slot: usize,
}

impl<'a, T> IterIndexed<'a, T> {
    pub(crate) const fn new(
        stamps: std::slice::Iter<'a, Stamp>,
        values: std::slice::Iter<'a, T>,
    ) -> Self {
        Self {
            stamps,
            values,
            slot: 0,
        }
    }
}

impl<'a, T> Iterator for IterIndexed<'a, T> {
    type Item = (Idx<T>, &'a T);

    fn next(&mut self) -> Option<Self::Item> {
        let stamp = *self.stamps.next()?;
        let value = self
            .values
            .next()
            .expect("arena value and stamp vectors have equal length");
        let idx = Idx::new(stamp, self.slot);
        self.slot += 1;
        Some((idx, value))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.values.size_hint()
    }
}

impl<T> ExactSizeIterator for IterIndexed<'_, T> {}
impl<T> std::iter::FusedIterator for IterIndexed<'_, T> {}

/// Mutable iterator yielding `(Idx<T>, &mut T)` pairs in allocation order.
pub struct IterIndexedMut<'a, T> {
    stamps: std::slice::Iter<'a, Stamp>,
    values: std::slice::IterMut<'a, T>,
    slot: usize,
}

impl<'a, T> IterIndexedMut<'a, T> {
    pub(crate) const fn new(
        stamps: std::slice::Iter<'a, Stamp>,
        values: std::slice::IterMut<'a, T>,
    ) -> Self {
        Self {
            stamps,
            values,
            slot: 0,
        }
    }
}

impl<'a, T> Iterator for IterIndexedMut<'a, T> {
    type Item = (Idx<T>, &'a mut T);

    fn next(&mut self) -> Option<Self::Item> {
        let stamp = *self.stamps.next()?;
        let value = self
            .values
            .next()
            .expect("arena value and stamp vectors have equal length");
        let idx = Idx::new(stamp, self.slot);
        self.slot += 1;
        Some((idx, value))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.values.size_hint()
    }
}

impl<T> ExactSizeIterator for IterIndexedMut<'_, T> {}
impl<T> std::iter::FusedIterator for IterIndexedMut<'_, T> {}

/// Draining iterator returned by [`Arena::drain`](crate::Arena::drain).
pub struct ArenaDrain<'a, T> {
    inner: std::vec::Drain<'a, T>,
}

impl<'a, T> ArenaDrain<'a, T> {
    pub(crate) const fn new(inner: std::vec::Drain<'a, T>) -> Self {
        Self { inner }
    }
}

impl<T> Iterator for ArenaDrain<'_, T> {
    type Item = T;

    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next()
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

impl<T> DoubleEndedIterator for ArenaDrain<'_, T> {
    fn next_back(&mut self) -> Option<Self::Item> {
        self.inner.next_back()
    }
}

impl<T> ExactSizeIterator for ArenaDrain<'_, T> {}
impl<T> std::iter::FusedIterator for ArenaDrain<'_, T> {}

/// Owning iterator produced by consuming an [`Arena`](crate::Arena).
pub struct ArenaIntoIter<T> {
    inner: std::vec::IntoIter<T>,
}

impl<T> ArenaIntoIter<T> {
    pub(crate) const fn new(inner: std::vec::IntoIter<T>) -> Self {
        Self { inner }
    }
}

impl<T> Iterator for ArenaIntoIter<T> {
    type Item = T;

    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next()
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

impl<T> DoubleEndedIterator for ArenaIntoIter<T> {
    fn next_back(&mut self) -> Option<Self::Item> {
        self.inner.next_back()
    }
}

impl<T> ExactSizeIterator for ArenaIntoIter<T> {}
impl<T> std::iter::FusedIterator for ArenaIntoIter<T> {}
