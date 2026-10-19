use super::{
    BranchRecvPollRequest, Context, NonNull, OperationHandle, OutSlot, Poll, RawRecvOutput,
    WaiterTransfer,
};
impl<'cfg, T> crate::runtime::SessionKit<'cfg, T>
where
    T: crate::transport::Transport + 'cfg,
{
    pub(super) unsafe fn reset_public_offer_state_raw(ptr: NonNull<()>, handle: OperationHandle) {
        let mut waiters = WaiterTransfer::empty();
        unsafe {
            // SAFETY: offer-state reset owns the carrier endpoint slot selected
            // by `handle` and clears only the resident public offer state.
            Self::with_public_endpoint_mut::<'cfg, _>(ptr, handle, (), |endpoint| {
                endpoint.reset_public_offer_state(&mut waiters);
            });
        }
    }

    pub(super) unsafe fn init_public_offer_state_raw(
        ptr: NonNull<()>,
        handle: OperationHandle,
    ) -> crate::endpoint::kernel::PublicOpLease {
        unsafe {
            // SAFETY: offer-state initialization owns the carrier endpoint slot
            // selected by `handle` and installs only the public offer state.
            Self::with_public_endpoint_mut::<'cfg, _>(
                ptr,
                handle,
                crate::endpoint::kernel::PublicOpLease::Rejected,
                |endpoint| endpoint.init_public_offer_state(),
            )
        }
    }

    pub(super) unsafe fn restore_public_route_branch_raw(
        ptr: NonNull<()>,
        handle: OperationHandle,
    ) {
        unsafe {
            // SAFETY: route-branch restore owns the carrier endpoint slot
            // selected by `handle` and restores the resident branch preview.
            Self::with_public_endpoint_mut::<'cfg, _>(ptr, handle, (), |endpoint| {
                endpoint.restore_public_route_branch();
            });
        }
    }

    pub(super) unsafe fn begin_public_branch_recv_state_raw(
        ptr: NonNull<()>,
        handle: OperationHandle,
    ) -> crate::endpoint::kernel::PublicOpLease {
        unsafe {
            // SAFETY: branch-recv decode initialization owns the carrier
            // endpoint slot selected by `handle` and arms only public decode
            // state.
            Self::with_public_endpoint_mut::<'cfg, _>(
                ptr,
                handle,
                crate::endpoint::kernel::PublicOpLease::Rejected,
                |kernel| kernel.begin_public_branch_recv_state(),
            )
        }
    }

    pub(super) unsafe fn reset_public_branch_recv_state_raw(
        ptr: NonNull<()>,
        handle: OperationHandle,
    ) {
        let mut waiters = WaiterTransfer::empty();
        unsafe {
            // SAFETY: branch-recv reset owns the carrier endpoint slot selected
            // by `handle` and clears only the resident branch-recv state.
            Self::with_public_endpoint_mut::<'cfg, _>(ptr, handle, (), |kernel| {
                kernel.reset_public_branch_recv_state(&mut waiters);
            });
        }
    }

    pub(super) unsafe fn poll_offer_public_endpoint(
        ptr: NonNull<()>,
        handle: OperationHandle,
        cx: &mut Context<'_>,
        out: *mut Poll<crate::endpoint::RecvResult<u8>>,
    ) {
        let mut waiters = WaiterTransfer::with_replacement(cx.waker());
        let poll = unsafe {
            // SAFETY: offer polling owns the carrier endpoint slot selected by
            // `handle`; the route label poll result is copied into the out slot
            // after the callback returns.
            Self::with_public_endpoint_mut::<'cfg, _>(
                ptr,
                handle,
                Poll::Ready(Err(crate::endpoint::RecvError::Transport(
                    crate::transport::TransportError::Failed,
                ))),
                |kernel| kernel.poll_public_offer(cx, &mut waiters),
            )
        };
        OutSlot::new(out).write(poll);
    }

    pub(super) unsafe fn poll_branch_recv_public_endpoint(request: BranchRecvPollRequest<'_, '_>) {
        let BranchRecvPollRequest {
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
            // SAFETY: branch recv polling owns the carrier endpoint slot
            // selected by `handle`; validation and payload staging stay inside
            // this decode poll callback.
            Self::with_public_endpoint_mut::<'cfg, _>(
                ptr,
                handle,
                Poll::Ready(Err(crate::endpoint::RecvError::Transport(
                    crate::transport::TransportError::Failed,
                ))),
                |kernel| match kernel.poll_public_branch_recv(
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
            // SAFETY: branch receive has committed and released its kernel
            // borrow; the consumed branch still retains the carrier Endpoint.
            unsafe { Self::wake_public_actor_waiters(ptr, handle.endpoint) };
        }
        OutSlot::new(out).write(poll);
    }
}
