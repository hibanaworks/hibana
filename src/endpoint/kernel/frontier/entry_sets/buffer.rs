use core::{
    ops::{Deref, DerefMut, Index, IndexMut},
    slice,
};

#[derive(Clone, Copy)]
pub(super) struct EntryView<T> {
    ptr: *const T,
    capacity: u16,
}

impl<T> EntryView<T> {
    pub(super) const EMPTY: Self = Self {
        ptr: core::ptr::null(),
        capacity: 0,
    };

    #[inline]
    pub(super) const unsafe fn from_parts(ptr: *const T, capacity: usize) -> Self {
        if capacity > u16::MAX as usize || (capacity != 0 && ptr.is_null()) {
            crate::invariant();
        }
        Self {
            ptr,
            capacity: capacity as u16,
        }
    }

    #[inline]
    pub(super) const fn capacity(&self) -> usize {
        self.capacity as usize
    }

    #[inline]
    pub(super) fn as_slice(&self) -> &[T] {
        if self.ptr.is_null() {
            &[]
        } else {
            /* SAFETY: only an unsafe owner constructor can create a nonempty
            view, and that owner guarantees an initialized resident slice for
            the complete use of this compact view. */
            unsafe { slice::from_raw_parts(self.ptr, self.capacity()) }
        }
    }
}

impl<T> Index<usize> for EntryView<T> {
    type Output = T;

    #[inline]
    fn index(&self, index: usize) -> &Self::Output {
        &self.as_slice()[index]
    }
}

pub(super) struct EntryBuffer<T> {
    ptr: *mut T,
    capacity: u16,
}

impl<T> EntryBuffer<T> {
    #[inline]
    pub(super) const fn capacity(&self) -> usize {
        self.capacity as usize
    }

    #[inline]
    pub(super) const unsafe fn from_parts(ptr: *mut T, capacity: usize) -> Self {
        if capacity > u16::MAX as usize || (capacity != 0 && ptr.is_null()) {
            crate::invariant();
        }
        Self {
            ptr,
            capacity: capacity as u16,
        }
    }

    #[inline]
    pub(super) const fn into_view(self) -> EntryView<T> {
        /* SAFETY: this buffer's unsafe constructor established the initialized
        backing slice. Consuming the mutation capability prevents an immutable view
        from coexisting with this buffer owner. */
        unsafe { EntryView::from_parts(self.ptr.cast_const(), self.capacity()) }
    }

    #[inline]
    pub(super) fn as_slice(&self) -> &[T] {
        if self.ptr.is_null() {
            &[]
        } else {
            /* SAFETY: the unsafe owner constructor established one initialized
            entry slice; shared slicing is tied to `&self`. */
            unsafe { slice::from_raw_parts(self.ptr, self.capacity()) }
        }
    }

    #[inline]
    pub(super) fn as_mut_slice(&mut self) -> &mut [T] {
        if self.ptr.is_null() {
            &mut []
        } else {
            /* SAFETY: `&mut self` is the entry-buffer mutation token, and the
            stored pointer/capacity describe its initialized resident slice. */
            unsafe { slice::from_raw_parts_mut(self.ptr, self.capacity()) }
        }
    }
}

impl<T> Deref for EntryBuffer<T> {
    type Target = [T];

    #[inline]
    fn deref(&self) -> &Self::Target {
        self.as_slice()
    }
}

impl<T> DerefMut for EntryBuffer<T> {
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.as_mut_slice()
    }
}

impl<T, I> Index<I> for EntryBuffer<T>
where
    [T]: Index<I>,
{
    type Output = <[T] as Index<I>>::Output;

    #[inline]
    fn index(&self, index: I) -> &Self::Output {
        &self.as_slice()[index]
    }
}

impl<T, I> IndexMut<I> for EntryBuffer<T>
where
    [T]: IndexMut<I>,
{
    #[inline]
    fn index_mut(&mut self, index: I) -> &mut Self::Output {
        &mut self.as_mut_slice()[index]
    }
}
