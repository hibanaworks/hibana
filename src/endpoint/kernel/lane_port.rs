//! Safe helpers for endpoint lane/port transport access.

use core::{
    ptr::NonNull,
    task::{Context, Poll},
};

use crate::{
    endpoint::{RecvError, RecvResult, SendError},
    rendezvous::port::Port,
    transport::{
        Outgoing, Transport, TransportError,
        wire::{Payload, WireEncode},
    },
};

pub(crate) use crate::rendezvous::port::{
    FrameMismatch, FrameObservation, PreambleFrame, PreambleObservation, ReceivedFrame,
};

#[derive(Clone, Copy)]
pub(super) struct FrameExpectation {
    pub(super) session_raw: u32,
    pub(super) lane_wire: u8,
    pub(super) source_role: u8,
    pub(super) target_role: u8,
    pub(super) label: u8,
}

#[derive(Clone, Copy)]
pub(crate) struct RawSendPayload {
    ptr: NonNull<()>,
    encode: unsafe fn(*const (), &mut [u8]) -> Result<usize, crate::transport::wire::CodecError>,
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
    fn port_key_for<'r, T>(port: &Port<'r, T>) -> *const ()
    where
        T: Transport + 'r,
    {
        core::ptr::from_ref(port).cast()
    }

    #[inline]
    fn begin_poll<'r, T>(&mut self, port: &Port<'r, T>)
    where
        T: Transport + 'r,
    {
        let port_key = Self::port_key_for(port);
        if self.port_key != Some(port_key) {
            self.clear();
        }
        if port.has_unresolved_recv_frame() {
            crate::invariant();
        }
        self.port_key = Some(port_key);
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SendIoState {
    Unpolled,
    TransportPending,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum SendCarrier {
    Local,
    Transport,
}

pub(super) struct PendingSend {
    payload: RawSendPayload,
    meta: crate::transport::SendMeta,
    carrier: SendCarrier,
    state: SendIoState,
}

impl PendingSend {
    #[inline]
    pub(super) const fn new(
        payload: RawSendPayload,
        meta: crate::transport::SendMeta,
        carrier: SendCarrier,
    ) -> Self {
        Self {
            payload,
            meta,
            carrier,
            state: SendIoState::Unpolled,
        }
    }

    #[inline]
    fn transport_is_pending(&self) -> bool {
        self.state == SendIoState::TransportPending
    }
}

impl RawSendPayload {
    #[inline(always)]
    pub(crate) fn from_typed<P: WireEncode>(payload: &P) -> Self {
        Self {
            ptr: NonNull::from(payload).cast(),
            encode: crate::transport::wire::erased_encoder::<P>(),
        }
    }

    #[inline(always)]
    pub(crate) fn encode_into(self, scratch: &mut [u8]) -> Result<usize, SendError> {
        let ptr = self.ptr.as_ptr().cast_const();
        // SAFETY: `from_typed` stores the encoder matching the erased payload
        // pointer, and the public send future moves that pair into staging once.
        let encoded_len = unsafe { (self.encode)(ptr, scratch) }.map_err(SendError::Codec)?;
        if encoded_len > scratch.len() {
            return Err(SendError::Codec(
                crate::transport::wire::CodecError::Malformed,
            ));
        }
        Ok(encoded_len)
    }
}

#[inline]
pub(super) fn frontier_scratch_ptr<'r, T>(port: &Port<'r, T>) -> *mut [u8]
where
    T: Transport + 'r,
{
    port.frontier_scratch_ptr()
}

/// # Safety
///
/// `payload` must point into endpoint-resident storage that remains valid for
/// the returned lifetime. Callers must not use this to extend a borrow from a
/// stack-local buffer, transport-owned pending buffer, or any storage that can be
/// retired before `'a` ends.
#[inline]
pub(crate) unsafe fn endpoint_resident_payload<'a>(payload: Payload<'_>) -> Payload<'a> {
    // SAFETY: caller guarantees the payload bytes live in endpoint-resident
    // storage for the returned lifetime.
    let bytes = unsafe { &*(payload.as_bytes() as *const [u8]) };
    Payload::new(bytes)
}

pub(super) fn poll_recv_frame<'r, T>(
    pending: &mut PendingRecv,
    port: &Port<'r, T>,
    expected: FrameExpectation,
    cx: &mut Context<'_>,
) -> Poll<RecvResult<ReceivedFrame<'r>>>
where
    T: Transport + 'r,
{
    let (payload, observed) = match poll_recv_payload(pending, port, cx) {
        Poll::Pending => return Poll::Pending,
        Poll::Ready(Ok(frame)) => frame,
        Poll::Ready(Err(err)) => return Poll::Ready(Err(RecvError::Transport(err))),
    };
    let Some(observation) = observed else {
        return Poll::Ready(Ok(ReceivedFrame::from_descriptor_checked_payload(
            port,
            payload,
            expected.source_role,
            expected.label,
        )));
    };
    if let Some(kind) = observation.mismatch_expected(
        expected.session_raw,
        expected.lane_wire,
        expected.source_role,
        expected.target_role,
        expected.label,
    ) {
        emit_transport_mismatch_observation(
            port,
            expected.session_raw,
            expected.lane_wire,
            FrameMismatch::new(observation, kind),
        );
        return Poll::Ready(Err(RecvError::PhaseInvariant));
    }
    let frame = PreambleFrame::from_accepted_payload(port, payload, observation);
    match frame.accept_parts(
        expected.session_raw,
        expected.target_role,
        expected.source_role,
        expected.label,
    ) {
        Ok(frame) => {
            emit_transport_frame_observation(port, observation);
            Poll::Ready(Ok(frame))
        }
        Err(mismatch) => {
            emit_transport_mismatch_observation(
                port,
                expected.session_raw,
                expected.lane_wire,
                mismatch,
            );
            Poll::Ready(Err(RecvError::PhaseInvariant))
        }
    }
}

pub(super) fn poll_recv_frame_preamble<'r, T>(
    pending: &mut PendingRecv,
    port: &Port<'r, T>,
    expected_session_raw: u32,
    expected_lane_wire: u8,
    expected_target_role: u8,
    cx: &mut Context<'_>,
) -> Poll<RecvResult<PreambleFrame<'r>>>
where
    T: Transport + 'r,
{
    let (payload, observed) = match poll_recv_payload(pending, port, cx) {
        Poll::Pending => return Poll::Pending,
        Poll::Ready(Ok(frame)) => frame,
        Poll::Ready(Err(err)) => return Poll::Ready(Err(RecvError::Transport(err))),
    };
    if let Some(observation) = observed
        && let Some(kind) = observation.mismatch_preamble(
            expected_session_raw,
            expected_lane_wire,
            expected_target_role,
        )
    {
        emit_transport_mismatch_observation(
            port,
            expected_session_raw,
            expected_lane_wire,
            FrameMismatch::new(observation, kind),
        );
        return Poll::Ready(Err(RecvError::PhaseInvariant));
    }
    match observed {
        Some(observed) => Poll::Ready(Ok(PreambleFrame::from_accepted_payload(
            port, payload, observed,
        ))),
        None => Poll::Ready(Ok(PreambleFrame::from_deterministic_payload(port, payload))),
    }
}

fn poll_recv_payload<'r, T>(
    pending: &mut PendingRecv,
    port: &Port<'r, T>,
    cx: &mut Context<'_>,
) -> Poll<Result<(Payload<'r>, Option<FrameObservation>), TransportError>>
where
    T: Transport + 'r,
{
    pending.begin_poll(port);
    let transport = port.transport();
    let rx_ptr = port.rx_ptr();
    let poll = unsafe {
        // SAFETY: `Port` owns the Rx handle for this lane. `PendingRecv`
        // enters a receive poll only when no frame receipt is unresolved and
        // serializes in-flight polls by port identity.
        transport.poll_recv(&mut *rx_ptr, cx)
    };
    match poll {
        Poll::Pending => Poll::Pending,
        Poll::Ready(Err(err)) => {
            pending.clear();
            Poll::Ready(Err(err))
        }
        Poll::Ready(Ok(received)) => {
            let observed = received
                .evidence()
                .frame_header()
                .map(FrameObservation::from_header);
            pending.clear();
            Poll::Ready(Ok((received.payload(), observed)))
        }
    }
}

#[cold]
#[inline(never)]
fn emit_transport_mismatch_observation<'r, T>(
    port: &Port<'r, T>,
    expected_session_raw: u32,
    expected_lane_wire: u8,
    mismatch: FrameMismatch,
) where
    T: Transport + 'r,
{
    let event = mismatch.tap_event(expected_session_raw, expected_lane_wire);
    crate::observe::core::emit(port.tap(), event);
}

#[cold]
#[inline(never)]
fn emit_transport_frame_observation<'r, T>(port: &Port<'r, T>, observation: FrameObservation)
where
    T: Transport + 'r,
{
    let event = crate::rendezvous::port::transport_frame_tap_event(observation);
    crate::observe::core::emit(port.tap(), event);
}

#[inline]
pub(super) fn poll_send_outgoing<'r, T>(
    pending: &mut PendingSend,
    port: &Port<'r, T>,
    cx: &mut Context<'_>,
) -> Poll<Result<(), SendError>>
where
    T: Transport + 'r,
{
    if pending.carrier == SendCarrier::Local {
        if pending.transport_is_pending() {
            crate::invariant();
        }
        let mut unit_payload = [];
        return Poll::Ready(pending.payload.encode_into(&mut unit_payload).map(|_| ()));
    }
    let Some(_scratch_lease) = port.try_scratch_lease() else {
        cx.waker().wake_by_ref();
        return Poll::Pending;
    };
    let scratch_ptr = port.scratch_ptr();
    let scratch = /* SAFETY: the scoped rendezvous scratch lease excludes
    reentrant scratch access and resident layout mutation for this encode/poll
    call. `Port` bounded the pointer and length to initialized slab bytes, and
    no reference to them escapes the transport call below. */ unsafe {
        &mut *scratch_ptr
    };
    let encoded_len = match pending.payload.encode_into(scratch) {
        Ok(encoded_len) => encoded_len,
        Err(err) => {
            if pending.transport_is_pending() {
                let transport = port.transport();
                let tx_ptr = port.tx_ptr();
                pending.state = SendIoState::Unpolled;
                unsafe {
                    // SAFETY: the pending state records an armed transport poll and this
                    // port owns its lane-local Tx handle. The state is detached before
                    // external cancellation, so reentry cannot consume it twice.
                    transport.cancel_send(&mut *tx_ptr);
                }
            }
            return Poll::Ready(Err(err));
        }
    };
    let outgoing = Outgoing {
        meta: pending.meta,
        payload: Payload::new(&scratch[..encoded_len]),
    };
    let transport = port.transport();
    let tx_ptr = port.tx_ptr();
    let poll = unsafe {
        // SAFETY: the scoped scratch lease keeps `outgoing` stable for this call,
        // and `Port` owns the lane-local Tx handle. The transport signature cannot
        // retain the shorter outgoing borrow in its longer-lived Tx state.
        transport.poll_send(&mut *tx_ptr, outgoing, cx)
    };
    match poll {
        Poll::Pending => {
            pending.state = SendIoState::TransportPending;
            Poll::Pending
        }
        Poll::Ready(result) => {
            pending.state = SendIoState::Unpolled;
            Poll::Ready(result.map_err(SendError::Transport))
        }
    }
}

#[inline]
pub(super) fn cancel_send_outgoing<'r, T>(pending: &mut PendingSend, port: &Port<'r, T>)
where
    T: Transport + 'r,
{
    if !pending.transport_is_pending() {
        return;
    }
    let transport = port.transport();
    let tx_ptr = port.tx_ptr();
    pending.state = SendIoState::Unpolled;
    unsafe {
        // SAFETY: the pending state records an armed transport poll and this
        // port owns its lane-local Tx handle. The state was detached before
        // external cancellation, preventing duplicate reentrant cleanup.
        transport.cancel_send(&mut *tx_ptr);
    }
}

#[cfg(test)]
mod tests {
    use super::RawSendPayload;
    use crate::{
        endpoint::SendError,
        transport::wire::{CodecError, WireEncode},
    };

    struct OversizedLength;

    impl WireEncode for OversizedLength {
        fn encode_into(&self, out: &mut [u8]) -> Result<usize, CodecError> {
            Ok(out.len() + 1)
        }
    }

    #[test]
    fn erased_encoder_rejects_length_beyond_scratch() {
        let payload = OversizedLength;
        let mut scratch = [0u8; 1];
        let result = RawSendPayload::from_typed(&payload).encode_into(&mut scratch);

        assert!(matches!(
            result,
            Err(SendError::Codec(CodecError::Malformed))
        ));
    }
}
