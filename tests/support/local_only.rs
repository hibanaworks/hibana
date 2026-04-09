use core::cell::UnsafeCell;

pub(crate) struct LocalCell<T> {
    value: UnsafeCell<T>,
}

impl<T> LocalCell<T> {
    pub const fn new(value: T) -> Self {
        Self {
            value: UnsafeCell::new(value),
        }
    }

    #[inline]
    pub fn with<R>(&self, f: impl FnOnce(&T) -> R) -> R {
        unsafe { f(&*self.value.get()) }
    }

    #[inline]
    pub fn with_mut<R>(&self, f: impl FnOnce(&mut T) -> R) -> R {
        unsafe { f(&mut *self.value.get()) }
    }
}
