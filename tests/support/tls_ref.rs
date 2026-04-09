use core::{cell::UnsafeCell, mem::MaybeUninit, ptr};
use std::thread::LocalKey;

pub(crate) fn with_tls_ref<T, R>(
    slot: &'static LocalKey<UnsafeCell<MaybeUninit<T>>>,
    init: impl FnOnce(*mut T),
    f: impl FnOnce(&'static T) -> R,
) -> R {
    slot.with(|cell| unsafe {
        let dst = (*cell.get()).as_mut_ptr();
        init(dst);
        let result = f(&*dst);
        ptr::drop_in_place(dst);
        result
    })
}
