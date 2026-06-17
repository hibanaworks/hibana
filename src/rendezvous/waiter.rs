use core::task::Waker;

pub(crate) struct WaiterSlot {
    waker: Option<Waker>,
}

impl WaiterSlot {
    #[inline]
    pub(crate) const fn empty() -> Self {
        Self { waker: None }
    }

    #[inline]
    pub(crate) unsafe fn init_empty(dst: *mut Self) {
        /* SAFETY: `WaiterSlot::init_empty` is called by table initializers with
        the destination slot still unpublished; writing `waker` to `None`
        initializes the complete one-field slot before any waiter borrow exists. */
        unsafe {
            dst.write(Self::empty());
        }
    }

    #[inline]
    pub(crate) unsafe fn init_owned(dst: *mut Self, waker: Waker) {
        /* SAFETY: `dst` is the caller-owned waiter slot for one table cell.
        `init_empty` initializes that cell, then `set_owned` installs the only
        live waker before the slot is exposed to route/association waiters. */
        unsafe {
            Self::init_empty(dst);
            (*dst).set_owned(waker);
        }
    }

    #[inline]
    pub(crate) unsafe fn init_clone_from(dst: *mut Self, source: &Self) {
        match source.waker.as_ref() {
            Some(waker) => {
                /* SAFETY: `dst` is a distinct waiter destination selected by
                the migrating owner; cloning the source waker initializes the
                destination slot without borrowing the source mutably. */
                unsafe {
                    Self::init_owned(dst, waker.clone());
                }
            }
            None => {
                /* SAFETY: `dst` is the unpublished waiter destination selected
                by the migrating owner, and the empty source state initializes
                the full slot as `None`. */
                unsafe {
                    Self::init_empty(dst);
                }
            }
        }
    }

    #[inline]
    pub(crate) fn set(&mut self, waker: &Waker) {
        self.clear();
        self.set_owned(waker.clone());
    }

    #[inline]
    pub(crate) fn set_owned(&mut self, waker: Waker) {
        self.waker = Some(waker);
    }

    #[inline]
    pub(crate) fn take(&mut self) -> Option<Waker> {
        self.waker.take()
    }

    #[inline]
    pub(crate) fn clear(&mut self) {
        self.waker = None;
    }

    #[inline]
    pub(crate) fn wake(&mut self) {
        if let Some(waker) = self.take() {
            waker.wake();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::WaiterSlot;
    use std::{
        cell::Cell,
        task::{RawWaker, RawWakerVTable, Waker},
    };

    unsafe fn clone_count_waker(data: *const ()) -> RawWaker {
        RawWaker::new(data, &COUNT_WAKER_VTABLE)
    }

    unsafe fn wake_count_waker(data: *const ()) {
        let count = /* SAFETY: `counting_waker` stores the address of the test
        `Cell<usize>` in this RawWaker; the initialized stack cell outlives
        every test waker, and this callback only performs `Cell` interior mutation. */ unsafe { &*data.cast::<Cell<usize>>() };
        count.set(count.get() + 1);
    }

    unsafe fn drop_count_waker(_: *const ()) {}

    static COUNT_WAKER_VTABLE: RawWakerVTable = RawWakerVTable::new(
        clone_count_waker,
        wake_count_waker,
        wake_count_waker,
        drop_count_waker,
    );

    fn counting_waker(count: &Cell<usize>) -> Waker {
        let data = core::ptr::from_ref(count).cast::<()>();
        /* SAFETY: `data` is the address of `count`, and every waker built in
        these tests is dropped before that initialized stack `Cell<usize>` goes out of
        scope; the vtable casts the same pointer back to `Cell<usize>`. */
        unsafe { Waker::from_raw(RawWaker::new(data, &COUNT_WAKER_VTABLE)) }
    }

    #[test]
    fn explicit_empty_slot_ignores_poisoned_storage_bytes() {
        let layout = std::alloc::Layout::new::<WaiterSlot>();
        let ptr =
            /* SAFETY: `layout` is exactly `WaiterSlot`; null is checked before
            the allocation is written or deallocated with the same layout. */
            unsafe { std::alloc::alloc(layout).cast::<WaiterSlot>() };
        if ptr.is_null() {
            std::alloc::handle_alloc_error(layout);
        }
        /* SAFETY: `ptr` is the unique allocation for this test's `WaiterSlot`.
        The poison bytes are overwritten by `init_empty`, and no reference to
        the slot is created until initialization returns. */
        unsafe {
            core::ptr::write_bytes(ptr.cast::<u8>(), 0x31, core::mem::size_of::<WaiterSlot>());
            WaiterSlot::init_empty(ptr);
        }

        let count = Cell::new(0);
        let waker = counting_waker(&count);
        /* SAFETY: this test owns `ptr` from allocation through deallocation;
        the mutable slot reference is the only reference before `drop_in_place`
        and uses the same `WaiterSlot` layout that allocated the cell. */
        unsafe {
            let slot = &mut *ptr;
            assert!(slot.take().is_none());
            slot.set(&waker);
            slot.wake();
            assert_eq!(count.get(), 1);
            assert!(slot.take().is_none());
            core::ptr::drop_in_place(ptr);
            std::alloc::dealloc(ptr.cast::<u8>(), layout);
        }
    }

    #[test]
    fn replacing_waiter_drops_displaced_waker_without_waking_it() {
        let first = Cell::new(0);
        let second = Cell::new(0);
        let first_waker = counting_waker(&first);
        let second_waker = counting_waker(&second);
        let mut slot = WaiterSlot::empty();

        slot.set(&first_waker);
        slot.set(&second_waker);
        slot.wake();

        assert_eq!(first.get(), 0);
        assert_eq!(second.get(), 1);
    }

    #[test]
    fn init_owned_transfers_waker_without_dropping_it() {
        let layout = std::alloc::Layout::new::<WaiterSlot>();
        let ptr =
            /* SAFETY: `layout` is exactly `WaiterSlot`; null is checked before
            `init_owned` writes the allocation or the same layout deallocates it. */
            unsafe { std::alloc::alloc(layout).cast::<WaiterSlot>() };
        if ptr.is_null() {
            std::alloc::handle_alloc_error(layout);
        }

        let count = Cell::new(0);
        let waker = counting_waker(&count);
        /* SAFETY: `ptr` is the unique `WaiterSlot` allocation from this test;
        `init_owned` initializes it once, and this block wakes, drops, and
        deallocates it exactly once with the matching layout. */
        unsafe {
            WaiterSlot::init_owned(ptr, waker);
            (*ptr).wake();
            assert_eq!(count.get(), 1);
            assert!((*ptr).take().is_none());
            core::ptr::drop_in_place(ptr);
            std::alloc::dealloc(ptr.cast::<u8>(), layout);
        }
    }
}
