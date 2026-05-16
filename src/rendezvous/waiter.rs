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
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
        task::{Wake, Waker},
    };

    struct CountWake(AtomicUsize);

    impl Wake for CountWake {
        fn wake(self: Arc<Self>) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }

        fn wake_by_ref(self: &Arc<Self>) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    fn counting_waker() -> (Arc<CountWake>, Waker) {
        let count = Arc::new(CountWake(AtomicUsize::new(0)));
        (Arc::clone(&count), Waker::from(count))
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

        let (count, waker) = counting_waker();
        unsafe {
            let slot = &mut *ptr;
            assert!(slot.take().is_none());
            slot.set(&waker);
            slot.wake();
            assert_eq!(count.0.load(Ordering::SeqCst), 1);
            assert!(slot.take().is_none());
            core::ptr::drop_in_place(ptr);
            std::alloc::dealloc(ptr.cast::<u8>(), layout);
        }
    }

    #[test]
    fn replacing_waiter_drops_previous_waker_without_waking_it() {
        let (first, first_waker) = counting_waker();
        let (second, second_waker) = counting_waker();
        let mut slot = WaiterSlot::empty();

        slot.set(&first_waker);
        slot.set(&second_waker);
        slot.wake();

        assert_eq!(first.0.load(Ordering::SeqCst), 0);
        assert_eq!(second.0.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn init_owned_transfers_waker_without_dropping_it() {
        let layout = std::alloc::Layout::new::<WaiterSlot>();
        let ptr = unsafe { std::alloc::alloc(layout).cast::<WaiterSlot>() };
        if ptr.is_null() {
            std::alloc::handle_alloc_error(layout);
        }

        let (count, waker) = counting_waker();
        unsafe {
            WaiterSlot::init_owned(ptr, waker);
            (*ptr).wake();
            assert_eq!(count.0.load(Ordering::SeqCst), 1);
            assert!((*ptr).take().is_none());
            core::ptr::drop_in_place(ptr);
            std::alloc::dealloc(ptr.cast::<u8>(), layout);
        }
    }
}
