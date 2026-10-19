use super::{Context, NonNull, OperationHandle, OutSlot, Poll, WaiterTransfer};
impl<'cfg, T> crate::runtime::SessionKit<'cfg, T>
where
    T: crate::transport::Transport + 'cfg,
{
    pub(super) unsafe fn init_public_send_state_raw(
        ptr: NonNull<()>,
        handle: OperationHandle,
        logical_label: u8,
        payload_kind: crate::eff::EventPayload,
    ) -> crate::endpoint::SendResult<()> {
        unsafe {
            // SAFETY: send-state initialization owns the carrier endpoint slot
            // selected by `handle`; only the typed message contract is stored.
            Self::with_public_endpoint_mut::<'cfg, _>(
                ptr,
                handle,
                Err(crate::endpoint::SendError::Transport(
                    crate::transport::TransportError::Failed,
                )),
                |kernel| kernel.init_public_send_state(logical_label, payload_kind),
            )
        }
    }

    pub(super) unsafe fn reset_public_send_state_raw(ptr: NonNull<()>, handle: OperationHandle) {
        let mut waiters = WaiterTransfer::empty();
        unsafe {
            // SAFETY: send-state reset owns the carrier endpoint slot selected
            // by `handle` and clears only the resident public send state.
            Self::with_public_endpoint_mut::<'cfg, _>(ptr, handle, (), |kernel| {
                kernel.reset_public_send_state(&mut waiters);
            });
        }
    }

    pub(super) unsafe fn poll_send_public_endpoint(
        ptr: NonNull<()>,
        handle: OperationHandle,
        payload: Option<crate::endpoint::kernel::lane_port::SendInput>,
        cx: &mut Context<'_>,
        out: *mut (),
    ) {
        let mut waiters = WaiterTransfer::with_replacement(cx.waker());
        let poll = unsafe {
            // SAFETY: send polling owns the carrier endpoint slot selected by
            // `handle`; the payload option is consumed inside this single poll
            // and the result is written to `out` after the callback returns.
            Self::with_public_endpoint_mut::<'cfg, _>(
                ptr,
                handle,
                Poll::Ready(Err(crate::endpoint::SendError::Transport(
                    crate::transport::TransportError::Failed,
                ))),
                |kernel| kernel.poll_public_send(cx, payload, &mut waiters),
            )
        };
        if poll.is_ready() {
            // SAFETY: the carrier borrow above ended; the send future still
            // retains this issued Endpoint and its runtime lifetime.
            unsafe { Self::wake_public_actor_waiters(ptr, handle.endpoint) };
        }
        OutSlot::erased::<
            Poll<crate::endpoint::SendResult<crate::endpoint::kernel::SendCommitOutcome<'cfg>>>,
        >(out)
        .write(poll);
    }
}
