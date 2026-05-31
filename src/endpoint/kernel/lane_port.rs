//! Safe helpers for endpoint lane/port transport access.

use core::{
    ptr::NonNull,
    task::{Context, Poll},
};

use crate::{
    binding::{BindingError, Channel, EndpointSlot},
    control::cap::mint::EpochTable,
    endpoint::SendError,
    rendezvous::port::Port,
    transport::{
        Outgoing, Transport, TransportError,
        wire::{Payload, WireEncode},
    },
};

pub(crate) use crate::rendezvous::port::ReceivedFrame;

#[derive(Clone, Copy)]
pub(crate) struct RawSendPayload {
    ptr: Option<NonNull<()>>,
}

pub(crate) struct PendingRecv {
    port_key: Option<*const ()>,
}

impl PendingRecv {
    #[inline]
    pub(super) const fn new() -> Self {
        Self { port_key: None }
    }

    #[inline]
    fn clear(&mut self) {
        self.port_key = None;
    }

    #[inline]
    fn port_key_for<'r, T, E>(port: &Port<'r, T, E>) -> *const ()
    where
        T: Transport + 'r,
        E: EpochTable + 'r,
    {
        core::ptr::from_ref(port).cast()
    }

    #[inline]
    pub(super) fn parks_port<'r, T, E>(&self, port: &Port<'r, T, E>) -> bool
    where
        T: Transport + 'r,
        E: EpochTable + 'r,
    {
        self.port_key == Some(Self::port_key_for(port))
    }

    #[inline]
    fn assert_no_unresolved_frame(&self) {
        assert!(
            self.port_key.is_none(),
            "transport receive frame polled while previous frame receipt is unresolved"
        );
    }
}

pub(super) struct PendingSend<'r> {
    outgoing: Option<Outgoing<'r>>,
}

impl<'r> PendingSend<'r> {
    #[inline]
    pub(super) const fn new() -> Self {
        Self { outgoing: None }
    }

    #[inline]
    fn clear(&mut self) {
        self.outgoing = None;
    }
}

impl RawSendPayload {
    #[inline(always)]
    pub(crate) fn from_typed<P: WireEncode>(payload: &P) -> Self {
        Self {
            ptr: Some(NonNull::from(payload).cast()),
        }
    }

    #[inline(always)]
    pub(crate) fn take(&mut self) -> Option<Self> {
        let payload = Self {
            ptr: Some(self.ptr?),
        };
        self.ptr = None;
        Some(payload)
    }

    #[inline(always)]
    pub(crate) fn encode_into(
        self,
        encode: crate::transport::wire::ErasedEncoder,
        scratch: &mut [u8],
    ) -> Result<usize, SendError> {
        let ptr = self
            .ptr
            .ok_or(SendError::PhaseInvariant)?
            .as_ptr()
            .cast_const();
        // SAFETY: the send runtime descriptor supplies the encoder matching
        // the projected message payload type that created this pointer.
        unsafe { encode(ptr, scratch) }.map_err(SendError::Codec)
    }
}

#[inline]
pub(super) fn scratch_ptr<'r, T, E>(port: &Port<'r, T, E>) -> *mut [u8]
where
    T: Transport + 'r,
    E: EpochTable + 'r,
{
    port.scratch_ptr()
}

#[inline]
pub(super) fn frontier_scratch_ptr<'r, T, E>(port: &Port<'r, T, E>) -> *mut [u8]
where
    T: Transport + 'r,
    E: EpochTable + 'r,
{
    port.frontier_scratch_ptr()
}

/// # Safety
///
/// `payload` must point into endpoint-resident storage that remains valid for
/// the returned lifetime. Callers must not use this to extend a borrow from a
/// stack temporary, transport-owned pending buffer, or any storage that can be
/// invalidated before `'a` ends.
#[inline]
pub(crate) unsafe fn endpoint_resident_payload<'a>(payload: Payload<'_>) -> Payload<'a> {
    // SAFETY: caller guarantees the payload bytes live in endpoint-resident
    // storage for the returned lifetime.
    let bytes = unsafe { &*(payload.as_bytes() as *const [u8]) };
    Payload::new(bytes)
}

/// # Safety
///
/// `binding_ptr` and `scratch_ptr` must be valid and uniquely borrowed for the
/// duration of the call. Both pointers must refer to endpoint-resident storage,
/// and the binding implementation may only return payload bytes whose borrow is
/// valid for the returned lifetime `'r`.
#[inline]
pub(super) unsafe fn recv_from_binding<'r, B: EndpointSlot + 'r>(
    binding_ptr: *mut B,
    channel: Channel,
    scratch_ptr: *mut [u8],
) -> Result<Payload<'r>, BindingError> {
    // SAFETY: caller guarantees both raw pointers are uniquely borrowed and
    // endpoint-resident for the duration of the binding callback.
    unsafe { (&mut *binding_ptr).on_recv(channel, &mut *scratch_ptr) }
}

#[inline]
pub(super) fn poll_recv<'r, T, E>(
    pending: &mut PendingRecv,
    port: &Port<'r, T, E>,
    cx: &mut Context<'_>,
) -> Poll<Result<Payload<'r>, TransportError>>
where
    T: Transport + 'r,
    E: EpochTable + 'r,
{
    let port_key = core::ptr::from_ref(port).cast();
    if pending.port_key != Some(port_key) {
        pending.clear();
    }
    pending.port_key = Some(port_key);
    let transport = port.transport();
    let rx_ptr = port.rx_ptr();
    let poll = unsafe {
        // SAFETY: `Port` owns the Rx handle for this lane. `PendingRecv`
        // serializes in-flight polls by port identity, and `poll_recv_frame`
        // issues a frame receipt before another received frame can be polled.
        transport.poll_recv(&mut *rx_ptr, cx)
    }
    .map_err(Into::into);
    if poll.is_ready() {
        pending.clear();
    }
    poll
}

#[inline]
pub(super) fn poll_recv_frame<'r, T, E>(
    pending: &mut PendingRecv,
    port: &Port<'r, T, E>,
    cx: &mut Context<'_>,
) -> Poll<Result<ReceivedFrame<'r>, TransportError>>
where
    T: Transport + 'r,
    E: EpochTable + 'r,
{
    match poll_recv(pending, port, cx) {
        Poll::Pending => Poll::Pending,
        Poll::Ready(Ok(payload)) => {
            pending.assert_no_unresolved_frame();
            Poll::Ready(Ok(ReceivedFrame::from_port(port, payload)))
        }
        Poll::Ready(Err(err)) => Poll::Ready(Err(err)),
    }
}

#[inline]
pub(super) fn begin_send_outgoing<'f, 'r>(pending: &mut PendingSend<'r>, outgoing: Outgoing<'f>)
where
    'r: 'f,
{
    pending.outgoing = Some(Outgoing {
        meta: outgoing.meta,
        payload: unsafe {
            // SAFETY: send planning encodes the erased user payload into the
            // endpoint-resident outgoing scratch before this `Outgoing` is staged.
            // `PendingSend` only extends that scratch borrow; completion,
            // cancellation, and send-state reset clear the scratch after the
            // transport no longer holds the outgoing view.
            endpoint_resident_payload(outgoing.payload)
        },
    });
}

#[inline]
pub(super) fn poll_send_outgoing<'r, T, E>(
    pending: &mut PendingSend<'r>,
    port: &Port<'r, T, E>,
    cx: &mut Context<'_>,
) -> Poll<Result<(), TransportError>>
where
    T: Transport + 'r,
    E: EpochTable + 'r,
{
    let outgoing = pending
        .outgoing
        .expect("pending send must be armed before polling");
    let transport = port.transport();
    let tx_ptr = port.tx_ptr();
    let poll = unsafe {
        // SAFETY: `PendingSend` owns the outgoing payload borrow while this send
        // is armed, and `Port` owns the lane-local Tx handle until the poll
        // completes or is explicitly cancelled.
        transport.poll_send(&mut *tx_ptr, outgoing, cx)
    }
    .map_err(Into::into);
    if poll.is_ready() {
        pending.clear();
    }
    poll
}

#[inline]
pub(super) fn cancel_send_outgoing<'r, T, E>(pending: &mut PendingSend<'r>, port: &Port<'r, T, E>)
where
    T: Transport + 'r,
    E: EpochTable + 'r,
{
    if pending.outgoing.is_none() {
        return;
    }
    let transport = port.transport();
    let tx_ptr = port.tx_ptr();
    unsafe {
        // SAFETY: a pending send is armed, so `PendingSend` owns the outgoing
        // payload and this `Port` owns the matching lane-local Tx handle.
        transport.cancel_send(&mut *tx_ptr);
    }
    pending.clear();
}

#[inline]
pub(super) fn requeue_recv_frame<'r, T, E>(
    port: &Port<'r, T, E>,
    frame: ReceivedFrame<'r>,
) -> Result<(), crate::transport::TransportError>
where
    T: Transport + 'r,
    E: EpochTable + 'r,
{
    frame.requeue_on(port)
}
