use super::{NonNull, OutSlot, PackedEndpointHandle, Poll, RawPayload, RecvPollRequest};
impl<'cfg, T> crate::runtime::SessionKit<'cfg, T>
where
    T: crate::transport::Transport + 'cfg,
{
    pub(super) unsafe fn init_public_recv_state_raw<const ROLE: u8>(
        ptr: NonNull<()>,
        handle: PackedEndpointHandle,
    ) -> crate::endpoint::kernel::PublicOpLease {
        unsafe {
            // SAFETY: this raw callback has exclusive access to the carrier
            // endpoint slot selected by `handle` for the duration of the call.
            Self::with_public_endpoint_mut::<'cfg, ROLE, _>(
                ptr,
                handle,
                crate::endpoint::kernel::PublicOpLease::Rejected,
                |kernel| kernel.init_public_recv_state(),
            )
        }
    }

    pub(super) unsafe fn reset_public_recv_state_raw<const ROLE: u8>(
        ptr: NonNull<()>,
        handle: PackedEndpointHandle,
    ) {
        unsafe {
            // SAFETY: this raw callback has exclusive access to the carrier
            // endpoint slot selected by `handle` for the duration of the call.
            Self::with_public_endpoint_mut::<'cfg, ROLE, _>(ptr, handle, (), |kernel| {
                kernel.reset_public_recv_state();
            });
        }
    }

    pub(super) unsafe fn poll_recv_public_endpoint<const ROLE: u8>(
        request: RecvPollRequest<'_, '_>,
    ) {
        let RecvPollRequest {
            ptr,
            handle,
            logical_label,
            validate,
            cx,
            out,
        } = request;
        let poll = unsafe {
            // SAFETY: this raw callback has exclusive access to the carrier
            // endpoint slot selected by `handle` for the duration of the call.
            Self::with_public_endpoint_mut::<'cfg, ROLE, _>(
                ptr,
                handle,
                Poll::Ready(Err(crate::endpoint::RecvError::Transport(
                    crate::transport::TransportError::Failed,
                ))),
                |kernel| match kernel.poll_public_recv(logical_label, validate, cx) {
                    Poll::Pending => Poll::Pending,
                    Poll::Ready(Ok(payload)) => Poll::Ready(Ok(RawPayload::from_payload(payload))),
                    Poll::Ready(Err(err)) => Poll::Ready(Err(err)),
                },
            )
        };
        OutSlot::new(out).write(poll);
    }
}
