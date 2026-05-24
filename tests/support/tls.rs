use core::{cell::UnsafeCell, mem::MaybeUninit, ptr};
use std::thread::LocalKey;

/// Initialize TLS-backed resident storage, run a test, then tear it down.
///
/// This intentionally exercises the unsafe resident initialization path. Tests
/// that do not need stable static storage should use `SessionKitStorage`
/// directly so the host-owned lifecycle remains the default path.
pub(crate) fn with_resident_tls_ref<T, R>(
    slot: &'static LocalKey<UnsafeCell<MaybeUninit<T>>>,
    init: impl FnOnce(&'static mut MaybeUninit<T>) -> &'static T,
    f: impl FnOnce(&'static T) -> R,
) -> R {
    slot.with(|cell| unsafe {
        let storage: &'static mut MaybeUninit<T> = &mut *cell.get();
        let dst = storage.as_mut_ptr();
        let value = init(storage);
        let result = f(value);
        ptr::drop_in_place(dst);
        result
    })
}

pub(crate) fn with_tls_mut<T, R>(
    slot: &'static LocalKey<UnsafeCell<MaybeUninit<T>>>,
    init: impl FnOnce(*mut T),
    f: impl FnOnce(&'static mut T) -> R,
) -> R {
    slot.with(|cell| unsafe {
        let dst = (*cell.get()).as_mut_ptr();
        init(dst);
        let result = f(&mut *dst);
        ptr::drop_in_place(dst);
        result
    })
}
