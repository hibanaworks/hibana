use core::{cell::UnsafeCell, task::Waker};

/// One wake owner for one published endpoint lease generation.
pub(crate) struct EndpointWaiter {
    waker: UnsafeCell<Option<Waker>>,
}

impl EndpointWaiter {
    #[inline]
    pub(crate) const fn empty() -> Self {
        Self {
            waker: UnsafeCell::new(None),
        }
    }

    /// Install `waker` and return the displaced owner without invoking either
    /// callback while the lease record is borrowed.
    #[inline]
    pub(crate) fn replace(&self, waker: Waker) -> Option<Waker> {
        // SAFETY: Hibana rendezvous are local-only. The endpoint carrier and
        // wake owner serialize callback-free moves through this initialized cell.
        unsafe { (&mut *self.waker.get()).replace(waker) }
    }

    /// Remove the current wake owner without invoking its callbacks.
    #[inline]
    pub(crate) fn take(&self) -> Option<Waker> {
        // SAFETY: the same local-only owner rule as `replace` serializes this
        // callback-free move from the initialized endpoint-lease record.
        unsafe { (&mut *self.waker.get()).take() }
    }

    #[inline]
    pub(crate) fn is_empty(&self) -> bool {
        // SAFETY: local-only lease operations serialize this initialized waiter
        // field inspection and never retain the reference across a Waker callback.
        unsafe { (&*self.waker.get()).is_none() }
    }
}

impl Drop for EndpointWaiter {
    fn drop(&mut self) {
        if self.waker.get_mut().is_some() {
            crate::invariant();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::EndpointWaiter;
    use std::{
        cell::Cell,
        task::{RawWaker, RawWakerVTable, Waker},
    };

    unsafe fn clone_count_waker(data: *const ()) -> RawWaker {
        RawWaker::new(data, &COUNT_WAKER_VTABLE)
    }

    unsafe fn wake_count_waker(data: *const ()) {
        // SAFETY: `counting_waker` stores the live test `Cell` pointer and all
        // generated Wakers are dropped before that stack cell.
        let count = unsafe { &*data.cast::<Cell<usize>>() };
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
        // SAFETY: the vtable interprets `data` as this live `Cell`, and the test
        // consumes every Waker before `count` leaves scope.
        unsafe { Waker::from_raw(RawWaker::new(data, &COUNT_WAKER_VTABLE)) }
    }

    #[test]
    fn replacement_moves_displaced_owner_out_of_the_lease_record() {
        let first = Cell::new(0);
        let second = Cell::new(0);
        let waiter = EndpointWaiter::empty();

        assert!(waiter.replace(counting_waker(&first)).is_none());
        let displaced = waiter.replace(counting_waker(&second));
        assert!(displaced.is_some());
        drop(displaced);
        assert_eq!(first.get(), 0);

        if let Some(waker) = waiter.take() {
            waker.wake();
        }
        assert_eq!(second.get(), 1);
        assert!(waiter.take().is_none());
    }

    #[test]
    fn drop_rejects_a_registered_wake_owner() {
        let count = Cell::new(0);
        let waiter = EndpointWaiter::empty();
        assert!(waiter.replace(counting_waker(&count)).is_none());

        let rejected = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(waiter)));
        assert!(rejected.is_err());
    }
}
