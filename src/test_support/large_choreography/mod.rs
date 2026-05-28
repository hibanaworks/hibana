pub(crate) mod fanout_program;
pub(crate) mod huge_program;
pub(crate) mod linear_program;
pub(crate) mod localside;
pub(crate) mod route_control_kinds;
pub(crate) mod route_localside;

#[inline(always)]
pub(crate) fn drive<F: core::future::Future>(mut future: F) -> F::Output {
    let mut cx = core::task::Context::from_waker(core::task::Waker::noop());
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
