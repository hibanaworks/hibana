use super::{Context, NonNull, OutSlot, PackedEndpointHandle, Poll};
impl<'cfg, T, C, const MAX_RV: usize> crate::runtime::SessionKit<'cfg, T, C, MAX_RV>
where
    T: crate::transport::Transport + 'cfg,
    C: crate::runtime_core::config::Clock + 'cfg,
{
    pub(super) unsafe fn preview_public_endpoint<const ROLE: u8>(
        ptr: NonNull<()>,
        handle: PackedEndpointHandle,
        logical_label: u8,
        out: *mut crate::endpoint::kernel::SendPreview,
    ) -> crate::endpoint::SendResult<()> {
        unsafe {
            // SAFETY: this raw callback has exclusive access to the carrier
            // endpoint slot selected by `handle` for the duration of the call.
            Self::with_public_endpoint_mut::<'_, ROLE, _>(
                ptr,
                handle,
                Err(crate::endpoint::SendError::Transport(
                    crate::transport::TransportError::Failed,
                )),
                |kernel| {
                    let preview = kernel.preview_flow_meta(logical_label)?;
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
    ) -> bool {
        let init = unsafe {
            // SAFETY: caller provides the initialized send-state descriptor for
            // this callback invocation.
            &*init
        };
        unsafe {
            // SAFETY: this raw callback has exclusive access to the carrier
            // endpoint slot selected by `handle` for the duration of the call.
            Self::with_public_endpoint_mut::<'cfg, ROLE, _>(ptr, handle, false, |kernel| {
                kernel.init_public_send_state(init)
            })
        }
    }

    pub(super) unsafe fn reset_public_send_state_raw<const ROLE: u8>(
        ptr: NonNull<()>,
        handle: PackedEndpointHandle,
    ) {
        unsafe {
            // SAFETY: this raw callback has exclusive access to the carrier
            // endpoint slot selected by `handle` for the duration of the call.
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
            // SAFETY: this raw callback has exclusive access to the carrier
            // endpoint slot selected by `handle` for the duration of the call.
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
