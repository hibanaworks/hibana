use core::{cell::UnsafeCell, mem::MaybeUninit, ptr};
use std::thread::LocalKey;

pub(crate) fn with_tls_ref<T, R>(
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
