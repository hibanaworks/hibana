use super::{Context, NonNull, OutSlot, PackedEndpointHandle, Poll};
impl<'cfg, T> crate::runtime::SessionKit<'cfg, T>
where
    T: crate::transport::Transport + 'cfg,
{
    pub(super) unsafe fn preview_public_send<const ROLE: u8>(
        ptr: NonNull<()>,
        handle: PackedEndpointHandle,
        logical_label: u8,
        out: *mut crate::endpoint::kernel::SendPreview,
    ) -> crate::endpoint::SendResult<()> {
        unsafe {
            // SAFETY: preview-send owns the carrier endpoint slot selected by
            // `handle` for this call and writes only the caller-provided
            // `SendPreview` out slot before returning.
            Self::with_public_endpoint_mut::<'_, ROLE, _>(
                ptr,
                handle,
                Err(crate::endpoint::SendError::Transport(
                    crate::transport::TransportError::Failed,
                )),
                |kernel| {
                    let preview = kernel.preview_send_meta(logical_label)?;
                    OutSlot::new(out).write(preview);
                    Ok(())
                },
            )
        }
    }

    pub(super) unsafe fn init_public_send_state_raw<const ROLE: u8>(
        ptr: NonNull<()>,
        handle: PackedEndpointHandle,
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
            Self::with_public_endpoint_mut::<'cfg, ROLE, _>(
                ptr,
                handle,
                crate::endpoint::kernel::PublicOpLease::Rejected,
                |kernel| kernel.init_public_send_state(init),
            )
        }
    }

    pub(super) unsafe fn reset_public_send_state_raw<const ROLE: u8>(
        ptr: NonNull<()>,
        handle: PackedEndpointHandle,
    ) {
        unsafe {
            // SAFETY: send-state reset owns the carrier endpoint slot selected
            // by `handle` and clears only the resident public send state.
            Self::with_public_endpoint_mut::<'cfg, ROLE, _>(ptr, handle, (), |kernel| {
                kernel.reset_public_send_state();
            });
        }
    }

    pub(super) unsafe fn poll_send_public_endpoint<const ROLE: u8>(
        ptr: NonNull<()>,
        handle: PackedEndpointHandle,
        payload: Option<crate::endpoint::kernel::RawSendPayload>,
        cx: &mut Context<'_>,
        out: *mut (),
    ) {
        let poll = unsafe {
            // SAFETY: send polling owns the carrier endpoint slot selected by
            // `handle`; the payload option is consumed inside this single poll
            // and the result is written to `out` after the callback returns.
            Self::with_public_endpoint_mut::<'cfg, ROLE, _>(
                ptr,
                handle,
                Poll::Ready(Err(crate::endpoint::SendError::Transport(
                    crate::transport::TransportError::Failed,
                ))),
                |kernel| kernel.poll_public_send(cx, payload),
            )
        };
        OutSlot::erased::<
            Poll<crate::endpoint::SendResult<crate::endpoint::kernel::SendCommitOutcome<'cfg>>>,
        >(out)
        .write(poll);
    }
}
