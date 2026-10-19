use super::{
    NonNull, OperationHandle, OutSlot, Poll, RawRecvOutput, RecvPollRequest, WaiterTransfer,
};
impl<'cfg, T> crate::runtime::SessionKit<'cfg, T>
where
    T: crate::transport::Transport + 'cfg,
{
    pub(super) unsafe fn init_public_recv_state_raw(
        ptr: NonNull<()>,
        handle: OperationHandle,
    ) -> crate::endpoint::kernel::PublicOpLease {
        unsafe {
            // SAFETY: recv-state initialization owns the carrier endpoint slot
            // selected by `handle` and installs only public recv state.
            Self::with_public_endpoint_mut::<'cfg, _>(
                ptr,
                handle,
                crate::endpoint::kernel::PublicOpLease::Rejected,
                |kernel| kernel.init_public_recv_state(),
            )
        }
    }

    pub(super) unsafe fn poll_recv_public_endpoint(request: RecvPollRequest<'_, '_>) {
        let RecvPollRequest {
            ptr,
            handle,
            logical_label,
            payload_kind,
            validate,
            cx,
            out,
        } = request;
        let mut waiters = WaiterTransfer::with_replacement(cx.waker());
        let poll = unsafe {
            // SAFETY: endpoint recv polling owns the carrier endpoint slot
            // selected by `handle`; validation and payload staging stay inside
            // this recv poll callback.
            Self::with_public_endpoint_mut::<'cfg, _>(
                ptr,
                handle,
                Poll::Ready(Err(crate::endpoint::RecvError::Transport(
                    crate::transport::TransportError::Failed,
                ))),
                |kernel| match kernel.poll_public_recv(
                    logical_label,
                    payload_kind,
                    validate,
                    cx,
                    &mut waiters,
                ) {
                    Poll::Pending => Poll::Pending,
                    Poll::Ready(Ok(payload)) => {
                        Poll::Ready(Ok(RawRecvOutput::from_kernel(payload)))
                    }
                    Poll::Ready(Err(err)) => Poll::Ready(Err(err)),
                },
            )
        };
        if poll.is_ready() {
            // SAFETY: the kernel borrow ended; this receive still retains its
            // carrier Endpoint while other logical views are notified.
            unsafe { Self::wake_public_actor_waiters(ptr, handle.endpoint) };
        }
        OutSlot::new(out).write(poll);
    }
}
