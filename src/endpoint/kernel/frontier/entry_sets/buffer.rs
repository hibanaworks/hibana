use core::ops::{Deref, DerefMut, Index, IndexMut};

#[derive(Clone, Copy)]
pub(super) struct EntryView<'a, T> {
    slots: &'a [T],
}

impl<'a, T> EntryView<'a, T> {
    pub(super) const fn empty() -> Self {
        Self { slots: &[] }
    }

    #[inline]
    pub(super) unsafe fn from_parts(ptr: *const T, capacity: usize) -> Self {
        if capacity > u16::MAX as usize || (capacity != 0 && ptr.is_null()) {
            crate::invariant();
        }
        let slots = if capacity == 0 {
            &[]
        } else {
            /* SAFETY: the caller grants an initialized immutable span for the
            complete lifetime represented by this view. */
            unsafe { core::slice::from_raw_parts(ptr, capacity) }
        };
        Self { slots }
    }

    #[inline]
    pub(super) const fn capacity(&self) -> usize {
        self.slots.len()
    }

    #[inline]
    pub(super) const fn as_slice(&self) -> &[T] {
        self.slots
    }
}

impl<T> Index<usize> for EntryView<'_, T> {
    type Output = T;

    #[inline]
    fn index(&self, index: usize) -> &Self::Output {
        &self.as_slice()[index]
    }
}

pub(super) struct EntryBuffer<'a, T> {
    slots: &'a mut [T],
}

impl<'a, T> EntryBuffer<'a, T> {
    #[inline]
    pub(super) const fn capacity(&self) -> usize {
        self.slots.len()
    }

    #[inline]
    pub(super) fn from_slice(slots: &'a mut [T]) -> Self {
        if slots.len() > u16::MAX as usize {
            crate::invariant();
        }
        Self { slots }
    }

    #[inline]
    pub(super) const fn into_view(self) -> EntryView<'a, T> {
        EntryView { slots: self.slots }
    }

    #[inline]
    pub(super) const fn as_slice(&self) -> &[T] {
        self.slots
    }

    #[inline]
    pub(super) fn as_mut_slice(&mut self) -> &mut [T] {
        self.slots
    }
}

impl<T> Deref for EntryBuffer<'_, T> {
    type Target = [T];

    #[inline]
    fn deref(&self) -> &Self::Target {
        self.as_slice()
    }
}

impl<T> DerefMut for EntryBuffer<'_, T> {
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.as_mut_slice()
    }
}

impl<T, I> Index<I> for EntryBuffer<'_, T>
where
    [T]: Index<I>,
{
    type Output = <[T] as Index<I>>::Output;

    #[inline]
    fn index(&self, index: I) -> &Self::Output {
        &self.as_slice()[index]
    }
}

impl<T, I> IndexMut<I> for EntryBuffer<'_, T>
where
    [T]: IndexMut<I>,
{
    #[inline]
    fn index_mut(&mut self, index: I) -> &mut Self::Output {
        &mut self.as_mut_slice()[index]
    }
}
