use core::cell::UnsafeCell;

const TEST_SLAB_CAPACITY: usize = 1_048_576;

std::thread_local! {
    static TEST_SLAB: UnsafeCell<[u8; TEST_SLAB_CAPACITY]> =
        const { UnsafeCell::new([0u8; TEST_SLAB_CAPACITY]) };
}

pub(crate) fn with_runtime_workspace<R>(f: impl FnOnce(&'static mut [u8]) -> R) -> R {
    TEST_SLAB.with(|slab| unsafe {
        let slab = &mut *slab.get();
        slab.fill(0);
        f(&mut *(slab.as_mut_slice() as *mut [u8]))
    })
}
