//! Receive-frame receipt authority for a lane port.
//!
//! This module owns the one-shot proof that a transport frame has been
//! received from a specific port/Rx handle and must be committed, requeued, or
//! explicitly discarded.

use core::cell::Cell;

use super::Port;
use crate::{
    control::cap::mint::EpochTable,
    observe::{core::TapEvent, events::RawEvent, ids},
    transport::{FrameHeader, Transport, wire::Payload},
};

const RECEIVED_FRAME_CONTRACT: &str =
    "received transport frame dropped without explicit commit, requeue, or discard";

pub(super) struct RecvFrameReceiptState {
    outstanding: Cell<bool>,
}

struct PortRecvFrameReceipt {
    port_key: *const (),
    state: *const RecvFrameReceiptState,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FrameObservation {
    session: u32,
    meta: u32,
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FrameMismatchKind {
    Session = ids::TRANSPORT_MISMATCH_SESSION,
    Lane = ids::TRANSPORT_MISMATCH_LANE,
    SourceRole = ids::TRANSPORT_MISMATCH_SOURCE_ROLE,
    PeerRole = ids::TRANSPORT_MISMATCH_PEER_ROLE,
    Label = ids::TRANSPORT_MISMATCH_LABEL,
}

impl FrameMismatchKind {
    #[inline(always)]
    pub(crate) const fn tap_reason(self) -> u8 {
        self as u8
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FrameMismatch {
    observation: FrameObservation,
    kind: FrameMismatchKind,
}

struct ReceivedFrameCore<'r> {
    payload: Payload<'r>,
    receipt: PortRecvFrameReceipt,
    observed_source_label: ObservedSourceLabel,
    source_label: u16,
    lane_wire: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ObservedSourceLabel(u32);

/// Transport frame whose session, lane, and receiving peer role are accepted.
///
/// This type deliberately does not expose payload commit APIs. Offer/passive
/// paths may stage it while route authority is still being resolved, but must
/// promote it to [`ReceivedFrame`] with the selected descriptor before endpoint
/// progress can consume the payload.
pub(crate) struct PreambleFrame<'r> {
    core: ReceivedFrameCore<'r>,
}

/// Transport frame received from a lane port and accepted by an endpoint descriptor.
///
/// The payload is accompanied by a one-shot receipt. Endpoint code must choose
/// exactly one terminal action: commit the frame into a payload, requeue it on
/// the same port/Rx handle, or explicitly discard it.
/// If the transport rejects requeue, the receipt is resolved as an explicit
/// discard and the caller must treat the returned error as terminal for that
/// frame.
///
/// Invariant: received transport frames must be committed, explicitly requeued, or explicitly discarded.
pub(crate) struct ReceivedFrame<'r> {
    core: ReceivedFrameCore<'r>,
}

impl RecvFrameReceiptState {
    #[inline]
    pub(super) const fn new() -> Self {
        Self {
            outstanding: Cell::new(false),
        }
    }

    #[inline]
    fn issue(&self, port_key: *const ()) -> PortRecvFrameReceipt {
        assert!(
            !self.outstanding.replace(true),
            "transport receive frame polled while previous frame receipt is unresolved",
        );
        PortRecvFrameReceipt {
            port_key,
            state: core::ptr::from_ref(self),
        }
    }

    #[inline]
    fn resolve(&self) {
        assert!(
            self.outstanding.get(),
            "transport receive frame receipt is no longer current",
        );
        self.outstanding.set(false);
    }

    #[inline]
    fn assert_current(&self) {
        assert!(
            self.outstanding.get(),
            "transport receive frame receipt is no longer current",
        );
    }

    #[inline]
    pub(super) fn has_outstanding(&self) -> bool {
        self.outstanding.get()
    }
}

impl PortRecvFrameReceipt {
    #[inline]
    const fn is_current(&self) -> bool {
        !self.state.is_null()
    }

    #[inline]
    fn resolve(&mut self) {
        if !self.state.is_null() {
            // SAFETY: receipt construction stores a valid pointer to the
            // port-local receipt state, and clearing `state` ensures one-shot
            // resolution.
            unsafe { &*self.state }.resolve();
            self.port_key = core::ptr::null();
            self.state = core::ptr::null();
        }
    }

    #[inline]
    fn assert_matches(&self, port_key: *const (), receipt_state: *const RecvFrameReceiptState) {
        if self.state.is_null() {
            return;
        }
        assert_eq!(
            self.port_key, port_key,
            "received transport frame requeued on a different endpoint port",
        );
        assert_eq!(
            self.state, receipt_state,
            "received transport frame requeued on a different Rx handle",
        );
        // SAFETY: the receipt stores a pointer to this port's receipt state.
        // `assert_matches` has just proven both the port identity and the
        // state pointer identity before reading the state.
        unsafe { &*self.state }.assert_current();
    }
}

impl FrameObservation {
    #[inline]
    pub(crate) const fn new(
        session_raw: u32,
        lane_wire: u8,
        source_role: u8,
        peer_role: u8,
        label: u8,
    ) -> Self {
        Self {
            session: session_raw,
            meta: ((lane_wire as u32) << 24)
                | ((source_role as u32) << 16)
                | ((peer_role as u32) << 8)
                | (label as u32),
        }
    }

    #[inline]
    pub(crate) const fn from_header(header: FrameHeader) -> Self {
        Self::new(
            header.session().raw(),
            header.lane(),
            header.source_role(),
            header.peer_role(),
            header.label().raw(),
        )
    }

    #[inline]
    pub(crate) const fn session_raw(self) -> u32 {
        self.session
    }

    #[inline]
    pub(crate) const fn meta(self) -> u32 {
        self.meta
    }

    #[inline]
    pub(crate) const fn lane_wire(self) -> u8 {
        (self.meta >> 24) as u8
    }

    #[inline]
    pub(crate) const fn source_role(self) -> u8 {
        (self.meta >> 16) as u8
    }

    #[inline]
    pub(crate) const fn peer_role(self) -> u8 {
        (self.meta >> 8) as u8
    }

    #[inline]
    pub(crate) const fn label_raw(self) -> u8 {
        self.meta as u8
    }

    #[inline(always)]
    pub(crate) const fn mismatch_expected(
        self,
        session_raw: u32,
        lane_wire: u8,
        source_role: u8,
        peer_role: u8,
        label: u8,
    ) -> Option<FrameMismatchKind> {
        if self.session_raw() != session_raw {
            Some(FrameMismatchKind::Session)
        } else if self.lane_wire() != lane_wire {
            Some(FrameMismatchKind::Lane)
        } else if self.source_role() != source_role {
            Some(FrameMismatchKind::SourceRole)
        } else if self.peer_role() != peer_role {
            Some(FrameMismatchKind::PeerRole)
        } else if self.label_raw() != label {
            Some(FrameMismatchKind::Label)
        } else {
            None
        }
    }

    #[inline(always)]
    pub(crate) const fn mismatch_preamble(
        self,
        session_raw: u32,
        lane_wire: u8,
        peer_role: u8,
    ) -> Option<FrameMismatchKind> {
        if self.session_raw() != session_raw {
            Some(FrameMismatchKind::Session)
        } else if self.lane_wire() != lane_wire {
            Some(FrameMismatchKind::Lane)
        } else if self.peer_role() != peer_role {
            Some(FrameMismatchKind::PeerRole)
        } else {
            None
        }
    }
}

impl FrameMismatch {
    #[inline]
    pub(crate) const fn new(observation: FrameObservation, kind: FrameMismatchKind) -> Self {
        Self { observation, kind }
    }

    #[inline]
    pub(crate) fn tap_event(
        self,
        now32: u32,
        expected_session_raw: u32,
        expected_lane_wire: u8,
    ) -> TapEvent {
        let reason = self.kind.tap_reason();
        RawEvent::new(now32, ids::TRANSPORT_MISMATCH)
            .with_causal_key(crate::observe::core::TapEvent::make_causal_key(
                expected_lane_wire,
                reason,
            ))
            .with_arg0(expected_session_raw)
            .with_arg1(self.observation.session_raw())
            .with_arg2(self.observation.meta())
    }
}

#[inline]
pub(crate) fn transport_frame_tap_event(now32: u32, observation: FrameObservation) -> TapEvent {
    RawEvent::new(now32, ids::TRANSPORT_FRAME)
        .with_arg0(observation.session_raw())
        .with_arg1(observation.meta())
        .with_arg2(0)
}

impl ObservedSourceLabel {
    #[inline]
    const fn from_observation(observation: FrameObservation) -> Self {
        Self(source_label(observation.source_role(), observation.label_raw()) as u32)
    }

    #[inline]
    const fn from_source_label(source_role: u8, label: u8) -> Self {
        Self(source_label(source_role, label) as u32)
    }

    #[inline]
    const fn source_role(self) -> u8 {
        (self.0 >> 8) as u8
    }

    #[inline]
    const fn label_raw(self) -> u8 {
        self.0 as u8
    }

    #[inline]
    const fn mismatch_expected(self, source_role: u8, label: u8) -> Option<FrameMismatchKind> {
        if self.source_role() != source_role {
            Some(FrameMismatchKind::SourceRole)
        } else if self.label_raw() != label {
            Some(FrameMismatchKind::Label)
        } else {
            None
        }
    }

    #[inline]
    const fn observation(self, session_raw: u32, lane_wire: u8, peer_role: u8) -> FrameObservation {
        FrameObservation::new(
            session_raw,
            lane_wire,
            self.source_role(),
            peer_role,
            self.label_raw(),
        )
    }
}

impl<'r> ReceivedFrameCore<'r> {
    #[inline]
    fn from_payload<T, E>(
        port: &Port<'r, T, E>,
        payload: Payload<'r>,
        source_role: u8,
        label: u8,
        observed_source_label: ObservedSourceLabel,
    ) -> Self
    where
        T: Transport + 'r,
        E: EpochTable + 'r,
    {
        Self {
            payload,
            receipt: port.recv_frame_receipt.issue(Port::port_key(port)),
            observed_source_label,
            source_label: source_label(source_role, label),
            lane_wire: port.lane().as_wire(),
        }
    }

    #[inline]
    const fn lane_idx(&self) -> usize {
        self.lane_wire as usize
    }

    #[inline]
    const fn lane_wire(&self) -> u8 {
        self.lane_wire
    }

    #[inline]
    const fn frame_label_raw(&self) -> u8 {
        self.source_label as u8
    }

    #[inline]
    const fn observed_frame_label_raw(&self) -> u8 {
        self.observed_source_label.label_raw()
    }

    #[inline]
    fn is_empty(&self) -> bool {
        self.payload.as_bytes().is_empty()
    }

    #[inline]
    fn validated_payload<E, F>(&self, validate: F) -> Result<Payload<'r>, E>
    where
        F: FnOnce(Payload<'r>) -> Result<(), E>,
    {
        validate(self.payload)?;
        Ok(self.payload)
    }

    #[inline]
    fn into_payload(mut self) -> Payload<'r> {
        self.consume_receipt();
        self.payload
    }

    #[inline]
    fn discard_uncommitted(mut self) {
        self.consume_receipt();
    }

    #[inline]
    fn requeue_on<T, E>(
        mut self,
        port: &Port<'r, T, E>,
    ) -> Result<(), crate::transport::TransportError>
    where
        T: Transport + 'r,
        E: EpochTable + 'r,
    {
        self.assert_matches_port(port);
        let transport = port.transport();
        let rx_ptr = port.rx_ptr();
        let result = unsafe {
            // SAFETY: the frame receipt was issued by this exact port/Rx handle
            // and `assert_matches_port` above proved the port identity,
            // receipt-state pointer, and outstanding receipt state before requeueing.
            transport.requeue(&mut *rx_ptr).map_err(Into::into)
        };
        match result {
            Ok(()) => {
                self.consume_receipt();
                Ok(())
            }
            Err(err) => {
                self.discard_after_failed_requeue();
                Err(err)
            }
        }
    }

    #[inline]
    fn discard_after_failed_requeue(&mut self) {
        self.consume_receipt();
    }

    #[inline]
    fn consume_receipt(&mut self) {
        self.receipt.resolve();
    }

    #[inline]
    fn assert_matches_port<T, E>(&self, port: &Port<'r, T, E>)
    where
        T: Transport + 'r,
        E: EpochTable + 'r,
    {
        assert_eq!(
            self.lane_wire(),
            port.lane().as_wire(),
            "received transport frame requeued on a different lane",
        );
        self.receipt.assert_matches(
            Port::port_key(port),
            core::ptr::from_ref(&port.recv_frame_receipt),
        );
    }
}

impl Drop for ReceivedFrameCore<'_> {
    fn drop(&mut self) {
        assert!(!self.receipt.is_current(), "{}", RECEIVED_FRAME_CONTRACT,);
    }
}

impl<'r> PreambleFrame<'r> {
    #[inline(always)]
    pub(crate) fn from_accepted_payload<T, E>(
        port: &Port<'r, T, E>,
        payload: Payload<'r>,
        observed: FrameObservation,
    ) -> Self
    where
        T: Transport + 'r,
        E: EpochTable + 'r,
    {
        let observed_source_label = ObservedSourceLabel::from_observation(observed);
        Self {
            core: ReceivedFrameCore::from_payload(
                port,
                payload,
                observed.source_role(),
                observed.label_raw(),
                observed_source_label,
            ),
        }
    }

    #[inline(always)]
    pub(crate) fn accept_parts(
        self,
        expected_session_raw: u32,
        expected_peer_role: u8,
        source_role: u8,
        frame_label: u8,
    ) -> Result<ReceivedFrame<'r>, FrameMismatch> {
        let mut core = self.core;
        if let Some(kind) = core
            .observed_source_label
            .mismatch_expected(source_role, frame_label)
        {
            let observation = core.observed_source_label.observation(
                expected_session_raw,
                core.lane_wire(),
                expected_peer_role,
            );
            core.discard_uncommitted();
            return Err(FrameMismatch::new(observation, kind));
        }
        core.source_label = source_label(source_role, frame_label);
        Ok(ReceivedFrame { core })
    }

    #[inline]
    pub(crate) const fn lane_idx(&self) -> usize {
        self.core.lane_idx()
    }

    #[inline]
    pub(crate) const fn lane_wire(&self) -> u8 {
        self.core.lane_wire()
    }

    #[inline]
    pub(crate) const fn observed_frame_label_raw(&self) -> u8 {
        self.core.observed_frame_label_raw()
    }

    #[inline]
    pub(crate) const fn observed_transport_frame(
        &self,
        session_raw: u32,
        lane_wire: u8,
        peer_role: u8,
    ) -> FrameObservation {
        self.core
            .observed_source_label
            .observation(session_raw, lane_wire, peer_role)
    }

    #[inline]
    pub(crate) fn discard_uncommitted(self) {
        self.core.discard_uncommitted();
    }

    #[inline]
    pub(crate) fn requeue_on<T, E>(
        self,
        port: &Port<'r, T, E>,
    ) -> Result<(), crate::transport::TransportError>
    where
        T: Transport + 'r,
        E: EpochTable + 'r,
    {
        self.core.requeue_on(port)
    }
}

#[inline(always)]
const fn source_label(source_role: u8, label: u8) -> u16 {
    ((source_role as u16) << 8) | (label as u16)
}

impl<'r> ReceivedFrame<'r> {
    #[inline(always)]
    pub(crate) fn from_descriptor_checked_payload<T, E>(
        port: &Port<'r, T, E>,
        payload: Payload<'r>,
        source_role: u8,
        label: u8,
    ) -> Self
    where
        T: Transport + 'r,
        E: EpochTable + 'r,
    {
        Self {
            core: ReceivedFrameCore::from_payload(
                port,
                payload,
                source_role,
                label,
                ObservedSourceLabel::from_source_label(source_role, label),
            ),
        }
    }

    #[inline]
    pub(crate) const fn lane_idx(&self) -> usize {
        self.core.lane_idx()
    }

    #[inline]
    pub(crate) const fn lane_wire(&self) -> u8 {
        self.core.lane_wire()
    }

    #[inline]
    pub(crate) const fn frame_label_raw(&self) -> u8 {
        self.core.frame_label_raw()
    }

    #[inline]
    pub(crate) fn is_empty(&self) -> bool {
        self.core.is_empty()
    }

    #[inline]
    pub(crate) fn validated_payload<E, F>(&self, validate: F) -> Result<Payload<'r>, E>
    where
        F: FnOnce(Payload<'r>) -> Result<(), E>,
    {
        self.core.validated_payload(validate)
    }

    #[inline]
    pub(crate) fn into_payload(self) -> Payload<'r> {
        self.core.into_payload()
    }

    #[inline]
    pub(crate) fn discard_uncommitted(self) {
        self.core.discard_uncommitted();
    }

    #[inline]
    pub(crate) fn requeue_on<T, E>(
        self,
        port: &Port<'r, T, E>,
    ) -> Result<(), crate::transport::TransportError>
    where
        T: Transport + 'r,
        E: EpochTable + 'r,
    {
        self.core.requeue_on(port)
    }
}
