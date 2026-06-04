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
        Incoming, Outgoing, Transport, TransportError,
        wire::{Payload, WireEncode},
    },
};

pub(crate) use crate::rendezvous::port::{
    FrameMismatch, FrameObservation, PreambleFrame, ReceivedFrame,
};

#[derive(Clone, Copy)]
pub(crate) struct RawSendPayload {
    ptr: Option<NonNull<()>>,
}

pub(crate) struct PendingRecv {
    port_key: Option<*const ()>,
}

struct RecvPollPermit;

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
    fn begin_poll<'r, T, E>(&mut self, port: &Port<'r, T, E>) -> RecvPollPermit
    where
        T: Transport + 'r,
        E: EpochTable + 'r,
    {
        let port_key = Self::port_key_for(port);
        if self.port_key != Some(port_key) {
            self.clear();
        }
        assert!(
            !port.has_unresolved_recv_frame(),
            "transport receive frame polled while previous frame receipt is unresolved"
        );
        self.port_key = Some(port_key);
        RecvPollPermit
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

#[inline(always)]
pub(super) fn poll_recv_frame<'r, T, E>(
    pending: &mut PendingRecv,
    port: &Port<'r, T, E>,
    expected_session_raw: u32,
    expected_lane_wire: u8,
    expected_source_role: u8,
    expected_peer_role: u8,
    expected_label: u8,
    cx: &mut Context<'_>,
) -> Poll<Result<ReceivedFrame<'r>, TransportError>>
where
    T: Transport + 'r,
    E: EpochTable + 'r,
{
    let incoming = match poll_recv_incoming(pending, port, cx) {
        Poll::Pending => return Poll::Pending,
        Poll::Ready(Ok(incoming)) => incoming,
        Poll::Ready(Err(err)) => return Poll::Ready(Err(err)),
    };
    let observed = incoming.header().map(FrameObservation::from_header);
    let observation = observed.unwrap_or_else(|| {
        FrameObservation::new(
            expected_session_raw,
            expected_lane_wire,
            expected_source_role,
            expected_peer_role,
            expected_label,
        )
    });
    if let Some(kind) = observation.mismatch_expected(
        expected_session_raw,
        expected_lane_wire,
        expected_source_role,
        expected_peer_role,
        expected_label,
    ) {
        emit_transport_mismatch_observation(
            port,
            expected_session_raw,
            expected_lane_wire,
            FrameMismatch::new(observation, kind),
        );
        cx.waker().wake_by_ref();
        return Poll::Pending;
    }
    Poll::Ready(Ok(ReceivedFrame::from_accepted_payload(
        port,
        incoming.payload(),
        observation,
        observed,
    )))
}

#[inline(always)]
pub(super) fn poll_recv_frame_preamble<'r, T, E>(
    pending: &mut PendingRecv,
    port: &Port<'r, T, E>,
    expected_session_raw: u32,
    expected_lane_wire: u8,
    expected_peer_role: u8,
    cx: &mut Context<'_>,
) -> Poll<Result<PreambleFrame<'r>, TransportError>>
where
    T: Transport + 'r,
    E: EpochTable + 'r,
{
    let incoming = match poll_recv_incoming(pending, port, cx) {
        Poll::Pending => return Poll::Pending,
        Poll::Ready(Ok(incoming)) => incoming,
        Poll::Ready(Err(err)) => return Poll::Ready(Err(err)),
    };
    let observed = incoming.header().map(FrameObservation::from_header);
    if let Some(observation) = observed
        && let Some(kind) = observation.mismatch_preamble(
            expected_session_raw,
            expected_lane_wire,
            expected_peer_role,
        )
    {
        emit_transport_mismatch_observation(
            port,
            expected_session_raw,
            expected_lane_wire,
            FrameMismatch::new(observation, kind),
        );
        cx.waker().wake_by_ref();
        return Poll::Pending;
    }
    Poll::Ready(Ok(PreambleFrame::from_accepted_payload(
        port,
        incoming.payload(),
        observed,
    )))
}

#[inline(always)]
fn poll_recv_incoming<'r, T, E>(
    pending: &mut PendingRecv,
    port: &Port<'r, T, E>,
    cx: &mut Context<'_>,
) -> Poll<Result<Incoming<'r>, TransportError>>
where
    T: Transport + 'r,
    E: EpochTable + 'r,
{
    let _permit = pending.begin_poll(port);
    let transport = port.transport();
    let rx_ptr = port.rx_ptr();
    let poll = unsafe {
        // SAFETY: `Port` owns the Rx handle for this lane. `PendingRecv`
        // creates `RecvPollPermit` only when no frame receipt is unresolved and
        // serializes in-flight polls by port identity.
        transport.poll_recv(&mut *rx_ptr, cx)
    }
    .map_err(Into::into);
    match poll {
        Poll::Pending => Poll::Pending,
        Poll::Ready(Err(err)) => {
            pending.clear();
            Poll::Ready(Err(err))
        }
        Poll::Ready(Ok(incoming)) => {
            pending.clear();
            Poll::Ready(Ok(incoming))
        }
    }
}

#[cold]
#[inline(never)]
fn emit_transport_mismatch_observation<'r, T, E>(
    port: &Port<'r, T, E>,
    expected_session_raw: u32,
    expected_lane_wire: u8,
    mismatch: FrameMismatch,
) where
    T: Transport + 'r,
    E: EpochTable + 'r,
{
    let event = mismatch.tap_event(port.now32(), expected_session_raw, expected_lane_wire);
    crate::observe::core::emit(port.tap(), event);
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
