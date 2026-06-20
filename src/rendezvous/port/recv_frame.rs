//! Receive-frame receipt authority for a lane port.
//!
//! This module owns the one-shot proof that a transport frame has been
//! received from a specific port/Rx handle and must be committed, requeued, or
//! explicitly discarded.

use super::Port;
use crate::{
    observe::{core::TapEvent, events, ids},
    transport::{FrameHeader, Transport, wire::Payload},
};

use super::super::recv_frame_receipt::PortRecvFrameReceipt;
pub(super) use super::super::recv_frame_receipt::RecvFrameReceiptState;

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
    TargetRole = ids::TRANSPORT_MISMATCH_PEER_ROLE,
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

#[derive(Clone, Copy)]
pub(crate) enum PreambleObservation {
    Framed(FrameObservation),
    Deterministic,
}

/// Transport frame whose session, lane, and receiving target role are accepted.
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

impl FrameObservation {
    #[inline]
    pub(crate) const fn new(
        session_raw: u32,
        lane_wire: u8,
        source_role: u8,
        target_role: u8,
        label: u8,
    ) -> Self {
        Self {
            session: session_raw,
            meta: ((lane_wire as u32) << 24)
                | ((source_role as u32) << 16)
                | ((target_role as u32) << 8)
                | (label as u32),
        }
    }

    #[inline]
    pub(crate) const fn from_header(header: FrameHeader) -> Self {
        Self::new(
            header.session().raw(),
            header.lane(),
            header.source_role(),
            header.target_role(),
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
    pub(crate) const fn target_role(self) -> u8 {
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
        target_role: u8,
        label: u8,
    ) -> Option<FrameMismatchKind> {
        if self.session_raw() != session_raw {
            Some(FrameMismatchKind::Session)
        } else if self.lane_wire() != lane_wire {
            Some(FrameMismatchKind::Lane)
        } else if self.source_role() != source_role {
            Some(FrameMismatchKind::SourceRole)
        } else if self.target_role() != target_role {
            Some(FrameMismatchKind::TargetRole)
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
        target_role: u8,
    ) -> Option<FrameMismatchKind> {
        if self.session_raw() != session_raw {
            Some(FrameMismatchKind::Session)
        } else if self.lane_wire() != lane_wire {
            Some(FrameMismatchKind::Lane)
        } else if self.target_role() != target_role {
            Some(FrameMismatchKind::TargetRole)
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
    pub(crate) const fn headerless_preamble(
        session_raw: u32,
        lane_wire: u8,
        target_role: u8,
    ) -> Self {
        Self {
            observation: FrameObservation::new(session_raw, lane_wire, 0, target_role, 0),
            kind: FrameMismatchKind::Label,
        }
    }

    #[inline]
    pub(crate) const fn label_mismatch(observation: FrameObservation) -> Self {
        Self {
            observation,
            kind: FrameMismatchKind::Label,
        }
    }

    #[inline]
    pub(crate) const fn source_label_mismatch(
        observation: FrameObservation,
        source_role: u8,
        label: u8,
    ) -> Self {
        let kind = if observation.source_role() != source_role {
            FrameMismatchKind::SourceRole
        } else if observation.label_raw() != label {
            FrameMismatchKind::Label
        } else {
            crate::invariant()
        };
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
        let observed = if self.kind == FrameMismatchKind::Session {
            self.observation.session_raw()
        } else {
            self.observation.meta()
        };
        events::raw_event(now32, ids::TRANSPORT_MISMATCH)
            .with_causal_key(crate::observe::core::TapEvent::make_causal_key(
                expected_lane_wire,
                reason,
            ))
            .with_arg0(expected_session_raw)
            .with_arg1(observed)
    }
}

#[inline]
pub(crate) fn transport_frame_tap_event(now32: u32, observation: FrameObservation) -> TapEvent {
    events::raw_event(now32, ids::TRANSPORT_FRAME)
        .with_arg0(observation.session_raw())
        .with_arg1(observation.meta())
}

impl ObservedSourceLabel {
    const DETERMINISTIC: u32 = 1 << 31;

    #[inline]
    const fn from_observation(observation: FrameObservation) -> Self {
        Self(source_label(observation.source_role(), observation.label_raw()) as u32)
    }

    #[inline]
    const fn deterministic() -> Self {
        Self(Self::DETERMINISTIC)
    }

    #[inline]
    const fn from_source_label(source_role: u8, label: u8) -> Self {
        Self(source_label(source_role, label) as u32)
    }

    #[inline]
    const fn preamble_observation(
        self,
        session_raw: u32,
        lane_wire: u8,
        target_role: u8,
    ) -> PreambleObservation {
        if self.is_deterministic() {
            PreambleObservation::Deterministic
        } else {
            PreambleObservation::Framed(self.observation(session_raw, lane_wire, target_role))
        }
    }

    #[inline]
    const fn is_deterministic(self) -> bool {
        (self.0 & Self::DETERMINISTIC) != 0
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
    const fn observation(
        self,
        session_raw: u32,
        lane_wire: u8,
        target_role: u8,
    ) -> FrameObservation {
        FrameObservation::new(
            session_raw,
            lane_wire,
            self.source_role(),
            target_role,
            self.label_raw(),
        )
    }
}

impl<'r> ReceivedFrameCore<'r> {
    #[inline]
    fn from_payload<T>(
        port: &Port<'r, T>,
        payload: Payload<'r>,
        source_role: u8,
        label: u8,
        observed_source_label: ObservedSourceLabel,
    ) -> Self
    where
        T: Transport + 'r,
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
    fn requeue_on<T>(mut self, port: &Port<'r, T>) -> Result<(), crate::transport::TransportError>
    where
        T: Transport + 'r,
    {
        self.assert_matches_port(port);
        let transport = port.transport();
        let rx_ptr = port.rx_ptr();
        let result = unsafe {
            // SAFETY: the frame receipt was issued by this exact port/Rx handle
            // and `assert_matches_port` above proved the port identity,
            // receipt-state pointer, and outstanding receipt state before requeueing.
            transport.requeue(&mut *rx_ptr)
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
    fn assert_matches_port<T>(&self, port: &Port<'r, T>)
    where
        T: Transport + 'r,
    {
        if self.lane_wire() != port.lane().as_wire() {
            crate::invariant();
        }
        self.receipt.assert_matches(
            Port::port_key(port),
            core::ptr::from_ref(&port.recv_frame_receipt),
        );
    }
}

impl Drop for ReceivedFrameCore<'_> {
    fn drop(&mut self) {
        if self.receipt.is_current() {
            crate::invariant();
        }
    }
}

impl<'r> PreambleFrame<'r> {
    #[inline(always)]
    pub(crate) fn from_accepted_payload<T>(
        port: &Port<'r, T>,
        payload: Payload<'r>,
        observed: FrameObservation,
    ) -> Self
    where
        T: Transport + 'r,
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
    pub(crate) fn from_deterministic_payload<T>(port: &Port<'r, T>, payload: Payload<'r>) -> Self
    where
        T: Transport + 'r,
    {
        Self {
            core: ReceivedFrameCore::from_payload(
                port,
                payload,
                0,
                0,
                ObservedSourceLabel::deterministic(),
            ),
        }
    }

    pub(crate) fn accept_parts(
        self,
        expected_session_raw: u32,
        expected_target_role: u8,
        source_role: u8,
        frame_label: u8,
    ) -> Result<ReceivedFrame<'r>, FrameMismatch> {
        let mut core = self.core;
        if !core.observed_source_label.is_deterministic()
            && let Some(kind) = core
                .observed_source_label
                .mismatch_expected(source_role, frame_label)
        {
            let observation = core.observed_source_label.observation(
                expected_session_raw,
                core.lane_wire(),
                expected_target_role,
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
    pub(crate) fn observed_frame_label_raw(&self) -> u8 {
        if self.core.observed_source_label.is_deterministic() {
            crate::invariant();
        }
        self.core.observed_frame_label_raw()
    }

    #[inline]
    pub(crate) fn preamble_observation(
        &self,
        session_raw: u32,
        lane_wire: u8,
        target_role: u8,
    ) -> PreambleObservation {
        self.core
            .observed_source_label
            .preamble_observation(session_raw, lane_wire, target_role)
    }

    #[inline]
    pub(crate) fn observed_transport_frame(
        &self,
        session_raw: u32,
        lane_wire: u8,
        target_role: u8,
    ) -> FrameObservation {
        match self.preamble_observation(session_raw, lane_wire, target_role) {
            PreambleObservation::Framed(observation) => observation,
            PreambleObservation::Deterministic => crate::invariant(),
        }
    }

    #[inline]
    pub(crate) const fn is_deterministic(&self) -> bool {
        self.core.observed_source_label.is_deterministic()
    }

    #[inline]
    pub(crate) fn discard_uncommitted(self) {
        self.core.discard_uncommitted();
    }

    #[inline]
    pub(crate) fn requeue_on<T>(
        self,
        port: &Port<'r, T>,
    ) -> Result<(), crate::transport::TransportError>
    where
        T: Transport + 'r,
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
    pub(crate) fn from_descriptor_checked_payload<T>(
        port: &Port<'r, T>,
        payload: Payload<'r>,
        source_role: u8,
        label: u8,
    ) -> Self
    where
        T: Transport + 'r,
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
    pub(crate) fn requeue_on<T>(
        self,
        port: &Port<'r, T>,
    ) -> Result<(), crate::transport::TransportError>
    where
        T: Transport + 'r,
    {
        self.core.requeue_on(port)
    }
}
