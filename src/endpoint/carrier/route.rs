use super::{Context, DecodePollRequest, NonNull, OutSlot, PackedEndpointHandle, Poll, RawPayload};
impl<'cfg, T, C, const MAX_RV: usize> crate::runtime::SessionKit<'cfg, T, C, MAX_RV>
where
    T: crate::transport::Transport + 'cfg,
    C: crate::runtime_core::config::Clock + 'cfg,
{
    pub(super) unsafe fn reset_public_offer_state_raw<const ROLE: u8>(
        ptr: NonNull<()>,
        handle: PackedEndpointHandle,
    ) {
        unsafe {
            // SAFETY: this raw callback has exclusive access to the carrier
            // endpoint slot selected by `handle` for the duration of the call.
            Self::with_public_endpoint_mut::<'cfg, ROLE, _>(ptr, handle, (), |endpoint| {
                endpoint.reset_public_offer_state();
            });
        }
    }

    pub(super) unsafe fn init_public_offer_state_raw<const ROLE: u8>(
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
                |endpoint| endpoint.init_public_offer_state(),
            )
        }
    }

    pub(super) unsafe fn restore_public_route_branch_raw<const ROLE: u8>(
        ptr: NonNull<()>,
        handle: PackedEndpointHandle,
    ) {
        unsafe {
            // SAFETY: this raw callback has exclusive access to the carrier
            // endpoint slot selected by `handle` for the duration of the call.
            Self::with_public_endpoint_mut::<'cfg, ROLE, _>(ptr, handle, (), |endpoint| {
                endpoint.restore_public_route_branch();
            });
        }
    }

    pub(super) unsafe fn begin_public_decode_state_raw<const ROLE: u8>(
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
                |kernel| kernel.begin_public_decode_state(),
            )
        }
    }

    pub(super) unsafe fn reset_public_decode_state_raw<const ROLE: u8>(
        ptr: NonNull<()>,
        handle: PackedEndpointHandle,
    ) {
        unsafe {
            // SAFETY: this raw callback has exclusive access to the carrier
            // endpoint slot selected by `handle` for the duration of the call.
            Self::with_public_endpoint_mut::<'cfg, ROLE, _>(ptr, handle, (), |kernel| {
                kernel.reset_public_decode_state();
            });
        }
    }

    pub(super) unsafe fn poll_offer_public_endpoint<const ROLE: u8>(
        ptr: NonNull<()>,
        handle: PackedEndpointHandle,
        cx: &mut Context<'_>,
        out: *mut Poll<crate::endpoint::RecvResult<u8>>,
    ) {
        let poll = unsafe {
            // SAFETY: this raw callback has exclusive access to the carrier
            // endpoint slot selected by `handle` for the duration of the call.
            Self::with_public_endpoint_mut::<'cfg, ROLE, _>(
                ptr,
                handle,
                Poll::Ready(Err(crate::endpoint::RecvError::Transport(
                    crate::transport::TransportError::Failed,
                ))),
                |kernel| kernel.poll_public_offer(cx),
            )
        };
        OutSlot::new(out).write(poll);
    }

    pub(super) unsafe fn poll_decode_public_endpoint<const ROLE: u8>(
        request: DecodePollRequest<'_, '_>,
    ) {
        let DecodePollRequest {
            ptr,
            handle,
            logical_label,
            validate,
            zero_payload,
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
                |kernel| match kernel.poll_public_decode(logical_label, validate, zero_payload, cx)
                {
                    Poll::Pending => Poll::Pending,
                    Poll::Ready(Ok(payload)) => Poll::Ready(Ok(RawPayload::from_payload(payload))),
                    Poll::Ready(Err(err)) => Poll::Ready(Err(err)),
                },
            )
        };
        OutSlot::new(out).write(poll);
    }
}
