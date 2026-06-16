use core::{cell::UnsafeCell, ops::Deref, ptr};
use std::thread::LocalKey;

pub(crate) trait ResidentSlot {
    type Target: 'static;
    type Guard<'a>: Deref<Target = Self::Target>
    where
        Self: 'a;

    fn init_slot(&mut self) -> Self::Guard<'_>;
    fn reset_slot(&mut self);
}

impl<'cfg, T> ResidentSlot for hibana::runtime::SessionKitStorage<'cfg, T>
where
    T: hibana::runtime::transport::Transport + 'cfg,
    'cfg: 'static,
{
    type Target = hibana::runtime::SessionKit<'cfg, T>;
    type Guard<'a>
        = &'a hibana::runtime::SessionKit<'cfg, T>
    where
        Self: 'a;

    fn init_slot(&mut self) -> Self::Guard<'_> {
        self.init()
    }

    fn reset_slot(&mut self) {
        unsafe {
            // SAFETY: the TLS helper calls this only after the resident borrow
            // has been dropped. Rewriting the storage object restores the
            // uninitialized owner state for the next test using the same TLS
            // slot.
            ptr::drop_in_place(self);
            ptr::write(self, Self::uninit());
        }
    }
}

/// Initialize TLS-backed resident storage, run a test, then reset the slot.
pub(crate) fn with_resident_tls_ref<S, R>(
    slot: &'static LocalKey<UnsafeCell<S>>,
    f: impl FnOnce(&'static S::Target) -> R,
) -> R
where
    S: ResidentSlot + 'static,
{
    slot.with(|cell| unsafe {
        let storage: &'static mut S = &mut *cell.get();
        let guard = storage.init_slot();
        let value = &*guard as *const S::Target;
        let result = f(&*value);
        drop(guard);
        storage.reset_slot();
        result
    })
}
