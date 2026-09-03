use crate::Idx;
use crate::segments::Identity;

/// Iterator yielding `(Idx<T>, &T)` pairs in allocation order.
pub struct IterIndexed<'a, T> {
    identity: Option<&'a Identity>,
    values: std::slice::Iter<'a, T>,
    slot: usize,
}

impl<'a, T> IterIndexed<'a, T> {
    pub(crate) const fn new(
        identity: Option<&'a Identity>,
        values: std::slice::Iter<'a, T>,
    ) -> Self {
        Self {
            identity,
            values,
            slot: 0,
        }
    }
}

impl<'a, T> Iterator for IterIndexed<'a, T> {
    type Item = (Idx<T>, &'a T);

    fn next(&mut self) -> Option<Self::Item> {
        let value = self.values.next()?;
        let identity = self
            .identity
            .expect("the identity is assigned whenever the arena holds values");
        let idx = Idx::new(identity.stamp_of(self.slot), self.slot);
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
    identity: Option<&'a Identity>,
    values: std::slice::IterMut<'a, T>,
    slot: usize,
}

impl<'a, T> IterIndexedMut<'a, T> {
    pub(crate) const fn new(
        identity: Option<&'a Identity>,
        values: std::slice::IterMut<'a, T>,
    ) -> Self {
        Self {
            identity,
            values,
            slot: 0,
        }
    }
}

impl<'a, T> Iterator for IterIndexedMut<'a, T> {
    type Item = (Idx<T>, &'a mut T);

    fn next(&mut self) -> Option<Self::Item> {
        let value = self.values.next()?;
        let identity = self
            .identity
            .expect("the identity is assigned whenever the arena holds values");
        let idx = Idx::new(identity.stamp_of(self.slot), self.slot);
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
