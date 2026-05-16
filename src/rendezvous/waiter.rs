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
        unsafe {
            dst.write(Self::empty());
        }
    }

    #[inline]
    pub(crate) unsafe fn init_owned(dst: *mut Self, waker: Waker) {
        unsafe {
            Self::init_empty(dst);
            (*dst).set_owned(waker);
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
        let _ = self.take();
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
        let count = unsafe { &*data.cast::<Cell<usize>>() };
        count.set(count.get() + 1);
    }

    unsafe fn drop_count_waker(data: *const ()) {
        core::hint::black_box(data);
    }

    static COUNT_WAKER_VTABLE: RawWakerVTable = RawWakerVTable::new(
        clone_count_waker,
        wake_count_waker,
        wake_count_waker,
        drop_count_waker,
    );

    fn counting_waker(count: &Cell<usize>) -> Waker {
        let data = core::ptr::from_ref(count).cast::<()>();
        unsafe { Waker::from_raw(RawWaker::new(data, &COUNT_WAKER_VTABLE)) }
    }

    #[test]
    fn explicit_empty_slot_ignores_poisoned_storage_bytes() {
        let layout = std::alloc::Layout::new::<WaiterSlot>();
        let ptr = unsafe { std::alloc::alloc(layout).cast::<WaiterSlot>() };
        if ptr.is_null() {
            std::alloc::handle_alloc_error(layout);
        }
        unsafe {
            core::ptr::write_bytes(ptr.cast::<u8>(), 0x31, core::mem::size_of::<WaiterSlot>());
            WaiterSlot::init_empty(ptr);
        }

        let count = Cell::new(0);
        let waker = counting_waker(&count);
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
    fn replacing_waiter_drops_previous_waker_without_waking_it() {
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
        let ptr = unsafe { std::alloc::alloc(layout).cast::<WaiterSlot>() };
        if ptr.is_null() {
            std::alloc::handle_alloc_error(layout);
        }

        let count = Cell::new(0);
        let waker = counting_waker(&count);
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
