use super::{Context, NonNull, OutSlot, PackedEndpointHandle, Payload, Poll, RawPayload};
impl<'cfg, T, U, C, const MAX_RV: usize> crate::integration::SessionKit<'cfg, T, U, C, MAX_RV>
where
    T: crate::transport::Transport + 'cfg,
    U: crate::runtime::consts::LabelUniverse + 'cfg,
    C: crate::runtime::config::Clock + 'cfg,
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
    ) -> bool {
        unsafe {
            // SAFETY: this raw callback has exclusive access to the carrier
            // endpoint slot selected by `handle` for the duration of the call.
            Self::with_public_endpoint_mut::<'cfg, ROLE, _>(ptr, handle, false, |endpoint| {
                endpoint.init_public_offer_state()
            })
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
    ) -> bool {
        unsafe {
            // SAFETY: this raw callback has exclusive access to the carrier
            // endpoint slot selected by `handle` for the duration of the call.
            Self::with_public_endpoint_mut::<'cfg, ROLE, _>(ptr, handle, false, |kernel| {
                kernel.begin_public_decode_state()
            })
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
        ptr: NonNull<()>,
        handle: PackedEndpointHandle,
        logical_label: u8,
        expects_control: bool,
        validate: for<'a> fn(Payload<'a>) -> Result<(), crate::transport::wire::CodecError>,
        synthetic: for<'a> fn(
            &'a mut [u8],
        ) -> Result<Payload<'a>, crate::transport::wire::CodecError>,
        cx: &mut Context<'_>,
        out: *mut Poll<crate::endpoint::RecvResult<RawPayload>>,
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
                |kernel| match kernel.poll_public_decode(
                    logical_label,
                    expects_control,
                    validate,
                    synthetic,
                    cx,
                ) {
                    Poll::Pending => Poll::Pending,
                    Poll::Ready(Ok(payload)) => Poll::Ready(Ok(RawPayload::from_payload(payload))),
                    Poll::Ready(Err(err)) => Poll::Ready(Err(err)),
                },
            )
        };
        OutSlot::new(out).write(poll);
    }
}
