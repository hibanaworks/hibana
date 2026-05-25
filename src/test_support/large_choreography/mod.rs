pub(crate) mod fanout_program;
pub(crate) mod huge_program;
pub(crate) mod linear_program;
pub(crate) mod localside;
pub(crate) mod route_control_kinds;
pub(crate) mod route_localside;

#[inline(always)]
fn noop_waker() -> core::task::Waker {
    unsafe fn clone(_: *const ()) -> core::task::RawWaker {
        core::task::RawWaker::new(core::ptr::null(), &VTABLE)
    }
    unsafe fn wake(_: *const ()) {}
    unsafe fn wake_by_ref(_: *const ()) {}
    unsafe fn drop(_: *const ()) {}

    static VTABLE: core::task::RawWakerVTable =
        core::task::RawWakerVTable::new(clone, wake, wake_by_ref, drop);

    // SAFETY: the large-choreography test futures never dereference the raw
    // waker data; every vtable operation ignores the null data pointer.
    unsafe { core::task::Waker::from_raw(core::task::RawWaker::new(core::ptr::null(), &VTABLE)) }
}

#[inline(always)]
pub(crate) fn drive<F: core::future::Future>(mut future: F) -> F::Output {
    let waker = noop_waker();
    let mut cx = core::task::Context::from_waker(&waker);
    // SAFETY: the local future is stack-pinned for the duration of this loop
    // and is not moved until it returns `Ready`.
    let mut future = unsafe { core::pin::Pin::new_unchecked(&mut future) };
    loop {
        match future.as_mut().poll(&mut cx) {
            core::task::Poll::Ready(output) => return output,
            core::task::Poll::Pending => core::hint::spin_loop(),
        }
    }
}
