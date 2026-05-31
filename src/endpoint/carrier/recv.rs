use super::{Context, NonNull, OutSlot, PackedEndpointHandle, Payload, Poll, RawPayload};
impl<'cfg, T, U, C, const MAX_RV: usize> crate::integration::SessionKit<'cfg, T, U, C, MAX_RV>
where
    T: crate::transport::Transport + 'cfg,
    U: crate::runtime::consts::LabelUniverse + 'cfg,
    C: crate::runtime::config::Clock + 'cfg,
{
    pub(super) unsafe fn init_public_recv_state_raw<const ROLE: u8>(
        ptr: NonNull<()>,
        handle: PackedEndpointHandle,
    ) -> bool {
        unsafe {
            // SAFETY: this raw callback has exclusive access to the carrier
            // endpoint slot selected by `handle` for the duration of the call.
            Self::with_public_endpoint_mut::<'cfg, ROLE, _>(ptr, handle, false, |kernel| {
                kernel.init_public_recv_state()
            })
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
        ptr: NonNull<()>,
        handle: PackedEndpointHandle,
        logical_label: u8,
        expects_control: bool,
        control: Option<crate::global::ControlDesc>,
        accepts_empty_payload: bool,
        validate: for<'a> fn(Payload<'a>) -> Result<(), crate::transport::wire::CodecError>,
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
                |kernel| match kernel.poll_public_recv(
                    logical_label,
                    expects_control,
                    control,
                    accepts_empty_payload,
                    validate,
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
