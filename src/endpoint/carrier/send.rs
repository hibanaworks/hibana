use super::{Context, EndpointHandle, NonNull, OutSlot, Poll, WaiterTransfer};
impl<'cfg, T> crate::runtime::SessionKit<'cfg, T>
where
    T: crate::transport::Transport + 'cfg,
{
    pub(super) unsafe fn preview_public_send(
        ptr: NonNull<()>,
        handle: EndpointHandle,
        logical_label: u8,
        payload_kind: crate::eff::EventPayload,
        out: *mut crate::endpoint::kernel::SendPreview,
    ) -> crate::endpoint::SendResult<()> {
        let mut waiters = WaiterTransfer::empty();
        unsafe {
            // SAFETY: preview-send owns the carrier endpoint slot selected by
            // `handle` for this call and writes only the caller-provided
            // `SendPreview` out slot before returning.
            Self::with_public_endpoint_mut::<'_, _>(
                ptr,
                handle,
                Err(crate::endpoint::SendError::Transport(
                    crate::transport::TransportError::Failed,
                )),
                |kernel| {
                    let preview =
                        kernel.preview_send_meta(logical_label, payload_kind, &mut waiters)?;
                    OutSlot::new(out).write(preview);
                    Ok(())
                },
            )
        }
    }

    pub(super) unsafe fn init_public_send_state_raw(
        ptr: NonNull<()>,
        handle: EndpointHandle,
        init: *const crate::endpoint::kernel::SendInit,
    ) -> crate::endpoint::kernel::PublicOpLease {
        let init = unsafe {
            // SAFETY: caller provides the initialized send-state descriptor for
            // this callback invocation.
            &*init
        };
        unsafe {
            // SAFETY: send-state initialization owns the carrier endpoint slot
            // selected by `handle`; `init` is read during this call and not
            // stored through the raw pointer.
            Self::with_public_endpoint_mut::<'cfg, _>(
                ptr,
                handle,
                crate::endpoint::kernel::PublicOpLease::Rejected,
                |kernel| kernel.init_public_send_state(init),
            )
        }
    }

    pub(super) unsafe fn reset_public_send_state_raw(ptr: NonNull<()>, handle: EndpointHandle) {
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
        handle: EndpointHandle,
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
        OutSlot::erased::<
            Poll<crate::endpoint::SendResult<crate::endpoint::kernel::SendCommitOutcome<'cfg>>>,
        >(out)
        .write(poll);
    }
}
