//! Public endpoint operation lifecycle: preview reset, terminal clear, waiter, and public poll entrypoints.

use core::task::{Poll, Waker};

use super::{
    core::{
        CursorEndpoint, DecodeRuntimeDesc, MaterializedRouteBranch, SendCommitOutcome, SendInit,
        SendState, StagedPayload, kernel_decode, kernel_recv, kernel_send,
    },
    inbox::PackedIngressEvidence,
    lane_port,
    offer::OfferState,
};
use crate::{
    binding::BindingSlot,
    control::cap::mint::{EpochTable, MintConfigMarker},
    control::types::Lane,
    endpoint::{RecvError, RecvResult, SendError, SendResult},
    rendezvous::SessionFaultKind,
    runtime::{config::Clock, consts::LabelUniverse},
    transport::{
        Transport,
        wire::{CodecError, Payload},
    },
};

impl<'r, const ROLE: u8, T, U, C, E, const MAX_RV: usize, Mint, B>
    CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>
where
    T: Transport + 'r,
    U: LabelUniverse,
    C: Clock,
    E: EpochTable,
    Mint: MintConfigMarker,
    B: BindingSlot,
{
    #[inline]
    pub(in crate::endpoint) fn restore_materialized_route_branch(
        &mut self,
        mut branch: MaterializedRouteBranch<'r>,
    ) {
        let binding_evidence = PackedIngressEvidence::take(&mut branch.binding_evidence);
        match branch.staged_payload {
            Some(StagedPayload::Binding { lane, payload }) => {
                if let Some(evidence) = binding_evidence {
                    debug_assert_eq!(lane, branch.binding_evidence_lane);
                    self.restore_binding_payload_for_lane(lane as usize, evidence, payload);
                } else {
                    debug_assert!(
                        false,
                        "binding staged payload must keep its evidence until restore"
                    );
                }
            }
            Some(StagedPayload::Transport { frame }) => {
                if let Some(evidence) = binding_evidence {
                    self.put_back_binding_for_lane(branch.binding_evidence_lane as usize, evidence);
                }
                let port = self.port_for_lane(frame.lane_idx());
                lane_port::requeue_recv_frame(port, frame);
            }
            None => {
                if let Some(evidence) = binding_evidence {
                    self.put_back_binding_for_lane(branch.binding_evidence_lane as usize, evidence);
                }
            }
        }
    }

    #[inline]
    pub(in crate::endpoint) fn reset_public_offer_state(&mut self) {
        self.clear_session_waiter();
        let mut state = core::mem::replace(&mut self.public_offer_state, OfferState::new());
        self.restore_detached_offer_state(&mut state);
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn restore_detached_offer_state(
        &mut self,
        state: &mut OfferState<'r>,
    ) {
        let rollback = state.take_rollback_items();
        for evidence in [
            rollback.carried_binding_evidence,
            rollback.stage_binding_evidence,
        ]
        .into_iter()
        .flatten()
        {
            let (lane_idx, evidence) = evidence.into_parts();
            self.put_back_binding_for_lane(lane_idx, evidence);
        }
        for payload in [
            rollback.carried_transport_payload,
            rollback.stage_transport_payload,
        ]
        .into_iter()
        .flatten()
        {
            let port = self.port_for_lane(payload.lane_idx());
            lane_port::requeue_recv_frame(port, payload);
        }
    }

    #[inline]
    pub(in crate::endpoint) fn terminal_clear_public_offer_state(&mut self) {
        self.clear_session_waiter();
        let mut state = core::mem::replace(&mut self.public_offer_state, OfferState::new());
        state.discard_terminal();
    }

    #[inline]
    pub(in crate::endpoint) fn restore_public_route_branch(&mut self) {
        if let Some(branch) = self.public_route_branch.take() {
            self.restore_materialized_route_branch(branch);
        }
    }

    #[inline]
    pub(in crate::endpoint) fn init_public_send_state(&mut self, init: &SendInit) {
        let (meta, preview_cursor_index) = init.preview.into_parts();
        self.public_send_state = SendState::Init {
            descriptor: init.descriptor,
            meta,
            preview_cursor_index: Some(preview_cursor_index),
            payload: None,
        };
    }

    #[inline]
    pub(in crate::endpoint) fn set_public_send_payload(
        &mut self,
        payload: Option<lane_port::RawSendPayload>,
    ) {
        if let SendState::Init {
            payload: staged, ..
        } = &mut self.public_send_state
        {
            *staged = payload;
        } else {
            debug_assert!(false, "send payload can only be staged after flow preview");
        }
    }

    #[inline]
    pub(in crate::endpoint) fn reset_public_send_state(&mut self) {
        self.clear_session_waiter();
        let state = core::mem::replace(&mut self.public_send_state, SendState::Done);
        self.cancel_detached_send_state(state);
    }

    #[inline]
    pub(in crate::endpoint) fn terminal_clear_public_send_state(&mut self) {
        self.reset_public_send_state();
    }

    #[inline]
    fn cancel_detached_send_state(&mut self, state: SendState<'r>) {
        if let SendState::Sending { mut pending, .. } = state {
            let lane_idx = pending.lane_idx();
            self.rollback_send_commit_plan(pending.commit_plan.take());
            let port = self.port_for_lane(lane_idx);
            lane_port::cancel_send_outgoing(&mut pending.transport, port);
        }
    }

    #[inline]
    pub(in crate::endpoint) fn init_public_recv_state(&mut self) {
        self.public_recv_state = super::recv::RecvState::new();
    }

    #[inline]
    pub(in crate::endpoint) fn reset_public_recv_state(&mut self) {
        self.clear_session_waiter();
        self.public_recv_state = super::recv::RecvState::new();
    }

    #[inline]
    pub(in crate::endpoint) fn terminal_clear_public_recv_state(&mut self) {
        self.clear_session_waiter();
        self.public_recv_state = super::recv::RecvState::new();
    }

    #[inline]
    pub(in crate::endpoint) fn begin_public_decode_state(&mut self) {
        if let Some(branch) = self.public_route_branch.take() {
            self.public_decode_state = super::decode::DecodeState::new(branch);
        } else {
            self.public_decode_state = super::decode::DecodeState::empty();
        }
    }

    #[inline]
    pub(in crate::endpoint) fn reset_public_decode_state(&mut self) {
        self.clear_session_waiter();
        if self.public_decode_state.restore_on_drop
            && let Some(branch) = self.public_decode_state.branch.take()
        {
            self.restore_materialized_route_branch(branch);
        }
        self.public_decode_state = super::decode::DecodeState::empty();
    }

    #[inline]
    pub(in crate::endpoint) fn terminal_clear_public_decode_state(&mut self) {
        self.clear_session_waiter();
        let mut state = core::mem::replace(
            &mut self.public_decode_state,
            super::decode::DecodeState::empty(),
        );
        state.discard_terminal();
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn session_fault(&self) -> Option<SessionFaultKind> {
        self.control
            .cluster()
            .and_then(|cluster| cluster.session_fault(self.public_rv, self.sid))
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn poison_session(
        &self,
        cause: SessionFaultKind,
    ) -> SessionFaultKind {
        self.control
            .cluster()
            .map(|cluster| cluster.poison_session(self.public_rv, self.sid, cause))
            .unwrap_or(cause)
    }

    #[inline]
    fn primary_physical_lane(&self) -> Lane {
        self.port_for_lane(self.primary_lane).lane
    }

    #[inline]
    fn register_session_waiter(&self, waker: &Waker) {
        let lane = self.primary_physical_lane();
        if let Some(cluster) = self.control.cluster() {
            cluster.register_session_waiter(self.public_rv, self.sid, lane, waker);
        }
    }

    #[inline]
    fn clear_session_waiter(&self) {
        let lane = self.primary_physical_lane();
        if let Some(cluster) = self.control.cluster() {
            cluster.clear_session_waiter(self.public_rv, self.sid, lane);
        }
    }

    #[inline]
    pub(in crate::endpoint) fn poison_for_recv_error(&self, error: &RecvError) -> SessionFaultKind {
        let cause = match error {
            RecvError::Transport(_) | RecvError::Binding(_) => SessionFaultKind::TransportClosed,
            RecvError::SessionFault(kind) => *kind,
            RecvError::Codec(_) => SessionFaultKind::DecodeFailed,
            RecvError::PhaseInvariant => SessionFaultKind::ProgressInvariantViolated,
            RecvError::LabelMismatch { .. }
            | RecvError::PeerMismatch { .. }
            | RecvError::PolicyAbort { .. } => SessionFaultKind::ProtocolViolation,
        };
        self.poison_session(cause)
    }

    #[inline]
    pub(in crate::endpoint) fn poison_for_send_error(&self, error: &SendError) -> SessionFaultKind {
        let cause = match error {
            SendError::Transport(_) => SessionFaultKind::TransportClosed,
            SendError::SessionFault(kind) => *kind,
            SendError::Codec(_)
            | SendError::LabelMismatch { .. }
            | SendError::PolicyAbort { .. } => SessionFaultKind::ProtocolViolation,
            SendError::PhaseInvariant => SessionFaultKind::ProgressInvariantViolated,
        };
        self.poison_session(cause)
    }

    #[inline]
    pub(in crate::endpoint) fn poll_public_offer(
        &mut self,
        cx: &mut core::task::Context<'_>,
    ) -> Poll<RecvResult<u8>> {
        if let Some(kind) = self.session_fault() {
            self.terminal_clear_public_offer_state();
            return Poll::Ready(Err(RecvError::SessionFault(kind)));
        }
        if let Some(branch) = self.public_route_branch.as_ref() {
            self.clear_session_waiter();
            return Poll::Ready(Ok(branch.label()));
        }
        let mut offer_state = core::mem::replace(&mut self.public_offer_state, OfferState::new());
        let poll = self.poll_offer_state(&mut offer_state, cx);
        match poll {
            Poll::Pending => {
                self.register_session_waiter(cx.waker());
                self.public_offer_state = offer_state;
                Poll::Pending
            }
            Poll::Ready(Ok(branch)) => {
                self.clear_session_waiter();
                self.public_offer_state = OfferState::new();
                debug_assert!(
                    self.public_route_branch.is_none(),
                    "public route branch slot must be empty before offer materializes a new branch"
                );
                if self.public_route_branch.is_some() {
                    branch.discard_terminal();
                    Poll::Ready(Err(RecvError::PhaseInvariant))
                } else {
                    let label = branch.label();
                    self.public_route_branch = Some(branch);
                    Poll::Ready(Ok(label))
                }
            }
            Poll::Ready(Err(err)) => {
                offer_state.discard_terminal();
                self.clear_session_waiter();
                self.public_offer_state = OfferState::new();
                let _ = self.poison_for_recv_error(&err);
                Poll::Ready(Err(err))
            }
        }
    }

    #[inline]
    pub(in crate::endpoint) fn poll_public_recv(
        &mut self,
        logical_label: u8,
        expects_control: bool,
        accepts_empty_payload: bool,
        validate: for<'a> fn(Payload<'a>) -> Result<(), CodecError>,
        cx: &mut core::task::Context<'_>,
    ) -> Poll<RecvResult<Payload<'r>>> {
        if let Some(kind) = self.session_fault() {
            self.terminal_clear_public_recv_state();
            return Poll::Ready(Err(RecvError::SessionFault(kind)));
        }
        let mut recv_state =
            core::mem::replace(&mut self.public_recv_state, super::recv::RecvState::new());
        match kernel_recv(
            self,
            logical_label,
            expects_control,
            accepts_empty_payload,
            validate,
            &mut recv_state,
            cx,
        ) {
            Poll::Pending => {
                self.register_session_waiter(cx.waker());
                self.public_recv_state = recv_state;
                Poll::Pending
            }
            Poll::Ready(result) => {
                self.clear_session_waiter();
                self.public_recv_state = super::recv::RecvState::new();
                match result {
                    Ok(payload) => Poll::Ready(Ok(payload)),
                    Err(err) => {
                        let _ = self.poison_for_recv_error(&err);
                        Poll::Ready(Err(err))
                    }
                }
            }
        }
    }

    #[inline]
    pub(in crate::endpoint) fn poll_public_decode(
        &mut self,
        logical_label: u8,
        expects_control: bool,
        validate: for<'a> fn(Payload<'a>) -> Result<(), crate::transport::wire::CodecError>,
        synthetic: for<'a> fn(
            &'a mut [u8],
        ) -> Result<Payload<'a>, crate::transport::wire::CodecError>,
        cx: &mut core::task::Context<'_>,
    ) -> Poll<RecvResult<Payload<'r>>> {
        if let Some(kind) = self.session_fault() {
            self.terminal_clear_public_decode_state();
            return Poll::Ready(Err(RecvError::SessionFault(kind)));
        }
        let mut decode_state = core::mem::replace(
            &mut self.public_decode_state,
            super::decode::DecodeState::empty(),
        );
        let Some(branch) = decode_state.branch() else {
            self.clear_session_waiter();
            self.public_decode_state = super::decode::DecodeState::empty();
            let err = RecvError::PhaseInvariant;
            let _ = self.poison_for_recv_error(&err);
            return Poll::Ready(Err(err));
        };
        let descriptor = DecodeRuntimeDesc::new(
            logical_label,
            crate::transport::FrameLabel::new(branch.branch_meta.frame_label),
            expects_control,
            validate,
            synthetic,
        );
        match kernel_decode(self, descriptor, &mut decode_state, cx) {
            Poll::Pending => {
                self.register_session_waiter(cx.waker());
                self.public_decode_state = decode_state;
                Poll::Pending
            }
            Poll::Ready(result) => match result {
                Ok(payload) => {
                    self.clear_session_waiter();
                    self.public_decode_state = super::decode::DecodeState::empty();
                    Poll::Ready(Ok(payload))
                }
                Err(err) => {
                    decode_state.discard_terminal();
                    self.clear_session_waiter();
                    self.public_decode_state = super::decode::DecodeState::empty();
                    let _ = self.poison_for_recv_error(&err);
                    Poll::Ready(Err(err))
                }
            },
        }
    }

    #[inline]
    pub(in crate::endpoint) fn poll_public_send(
        &mut self,
        cx: &mut core::task::Context<'_>,
    ) -> Poll<SendResult<SendCommitOutcome<'r>>>
    where
        <Mint as MintConfigMarker>::Policy: crate::control::cap::mint::AllowsEndpointMint,
    {
        if let Some(kind) = self.session_fault() {
            self.reset_public_send_state();
            return Poll::Ready(Err(SendError::SessionFault(kind)));
        }
        let mut send_state = core::mem::replace(&mut self.public_send_state, SendState::Done);
        match kernel_send(self, &mut send_state, cx) {
            Poll::Pending => {
                self.register_session_waiter(cx.waker());
                self.public_send_state = send_state;
                Poll::Pending
            }
            Poll::Ready(result) => {
                self.clear_session_waiter();
                self.public_send_state = SendState::Done;
                match result {
                    Ok(outcome) => Poll::Ready(Ok(outcome)),
                    Err(err) => {
                        let _ = self.poison_for_send_error(&err);
                        Poll::Ready(Err(err))
                    }
                }
            }
        }
    }
}
