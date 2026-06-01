//! Public endpoint operation lifecycle: preview reset, terminal clear, and waiter ownership.

use core::task::Waker;

use super::{
    core::{
        CursorEndpoint, EndpointRevocationTerminal, MaterializedRouteBranch, PublicActiveOp,
        SendInit, SendState, StagedPayload,
    },
    inbox::PackedIngressEvidence,
    lane_port,
    offer::OfferState,
};
use crate::{
    binding::EndpointSlot,
    control::cap::mint::{EpochTable, MintConfigMarker},
    control::types::Lane,
    endpoint::{RecvError, SendError},
    rendezvous::SessionFaultKind,
    runtime::{config::Clock, consts::LabelUniverse},
    transport::Transport,
};

impl<'r, const ROLE: u8, T, U, C, E, const MAX_RV: usize, Mint, B>
    CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>
where
    T: Transport + 'r,
    U: LabelUniverse,
    C: Clock,
    E: EpochTable,
    Mint: MintConfigMarker,
    B: EndpointSlot,
{
    #[inline]
    pub(in crate::endpoint::kernel) fn public_op_busy_fault(&mut self) {
        self.public_active_op = PublicActiveOp::Poisoned;
        let _ = self.poison_session(SessionFaultKind::ProgressInvariantViolated);
    }

    #[inline]
    #[must_use]
    pub(in crate::endpoint::kernel) fn start_public_op(&mut self, op: PublicActiveOp) -> bool {
        match self.public_active_op {
            PublicActiveOp::Idle => {
                self.public_active_op = op;
                true
            }
            _ => {
                self.public_op_busy_fault();
                false
            }
        }
    }

    #[inline]
    #[must_use]
    fn transition_public_op(&mut self, from: PublicActiveOp, to: PublicActiveOp) -> bool {
        match self.public_active_op {
            current if current == from => {
                self.public_active_op = to;
                true
            }
            PublicActiveOp::Idle => {
                self.public_op_busy_fault();
                false
            }
            _ => {
                self.public_op_busy_fault();
                false
            }
        }
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn finish_public_op(&mut self, op: PublicActiveOp) {
        match self.public_active_op {
            current if current == op => {
                self.public_active_op = PublicActiveOp::Idle;
            }
            PublicActiveOp::Poisoned => {}
            _ => self.public_op_busy_fault(),
        }
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn clear_public_op_if_current(&mut self, op: PublicActiveOp) {
        if self.public_active_op == op {
            self.public_active_op = PublicActiveOp::Idle;
        }
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn clear_public_op_terminal(&mut self) {
        self.public_active_op = PublicActiveOp::Idle;
    }

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
                if lane_port::requeue_recv_frame(port, frame).is_err() {
                    let _ = self.poison_session(SessionFaultKind::TransportClosed);
                }
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
        self.finish_public_op(PublicActiveOp::Offer);
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
            if lane_port::requeue_recv_frame(port, payload).is_err() {
                let _ = self.poison_session(SessionFaultKind::TransportClosed);
            }
        }
    }

    #[inline]
    pub(in crate::endpoint) fn terminal_clear_public_offer_state(&mut self) {
        self.clear_session_waiter();
        self.clear_public_op_if_current(PublicActiveOp::Offer);
        let mut state = core::mem::replace(&mut self.public_offer_state, OfferState::new());
        state.discard_terminal();
    }

    #[inline]
    #[must_use]
    pub(in crate::endpoint) fn init_public_offer_state(&mut self) -> bool {
        if !self.start_public_op(PublicActiveOp::Offer) {
            return false;
        }
        self.public_offer_state = OfferState::new();
        true
    }

    #[inline]
    pub(in crate::endpoint) fn revoke_clear_public_offer_state(&mut self) {
        self.clear_public_op_if_current(PublicActiveOp::Offer);
        let mut state = core::mem::replace(&mut self.public_offer_state, OfferState::new());
        state.discard_terminal();
    }

    #[inline]
    pub(in crate::endpoint) fn restore_public_route_branch(&mut self) {
        self.finish_public_op(PublicActiveOp::RouteBranch);
        if let Some(branch) = self.public_route_branch.take() {
            self.restore_materialized_route_branch(branch);
        }
    }

    #[inline]
    #[must_use]
    pub(in crate::endpoint) fn init_public_send_state(&mut self, init: &SendInit) -> bool {
        if !self.start_public_op(PublicActiveOp::Send) {
            return false;
        }
        let (meta, preview_cursor_index) = init.preview.into_parts();
        self.public_send_state = SendState::Init {
            descriptor: init.descriptor,
            meta,
            preview_cursor_index: Some(preview_cursor_index),
        };
        true
    }

    #[inline]
    pub(in crate::endpoint) fn reset_public_send_state(&mut self) {
        self.clear_session_waiter();
        self.finish_public_op(PublicActiveOp::Send);
        let state = core::mem::replace(&mut self.public_send_state, SendState::Done);
        self.cancel_detached_send_state(state);
    }

    #[inline]
    pub(in crate::endpoint) fn terminal_clear_public_send_state(&mut self) {
        self.clear_session_waiter();
        self.clear_public_op_if_current(PublicActiveOp::Send);
        let state = core::mem::replace(&mut self.public_send_state, SendState::Done);
        self.cancel_detached_send_state(state);
    }

    #[inline]
    pub(in crate::endpoint) fn revoke_drain_public_send_terminal(
        &mut self,
        terminal: &mut EndpointRevocationTerminal<'r>,
    ) {
        if let SendState::Sending { pending, .. } = &mut self.public_send_state
            && let Some(plan) = pending.commit_plan.take()
        {
            terminal.set_send_plan(plan);
        }
    }

    #[inline]
    pub(in crate::endpoint) fn revoke_finish_public_send_state(&mut self) {
        self.clear_public_op_if_current(PublicActiveOp::Send);
        let state = core::mem::replace(&mut self.public_send_state, SendState::Done);
        self.cancel_detached_send_state(state);
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
    #[must_use]
    pub(in crate::endpoint) fn init_public_recv_state(&mut self) -> bool {
        if !self.start_public_op(PublicActiveOp::Recv) {
            return false;
        }
        self.public_recv_state = super::recv::RecvState::new();
        true
    }

    #[inline]
    pub(in crate::endpoint) fn reset_public_recv_state(&mut self) {
        self.clear_session_waiter();
        self.finish_public_op(PublicActiveOp::Recv);
        self.public_recv_state = super::recv::RecvState::new();
    }

    #[inline]
    pub(in crate::endpoint) fn terminal_clear_public_recv_state(&mut self) {
        self.clear_session_waiter();
        self.clear_public_op_if_current(PublicActiveOp::Recv);
        self.public_recv_state = super::recv::RecvState::new();
    }

    #[inline]
    pub(in crate::endpoint) fn revoke_clear_public_recv_state(&mut self) {
        self.clear_public_op_if_current(PublicActiveOp::Recv);
        self.public_recv_state = super::recv::RecvState::new();
    }

    #[inline]
    #[must_use]
    pub(in crate::endpoint) fn begin_public_decode_state(&mut self) -> bool {
        if !self.transition_public_op(PublicActiveOp::RouteBranch, PublicActiveOp::Decode) {
            self.public_decode_state = super::decode::DecodeState::empty();
            return false;
        }
        if let Some(branch) = self.public_route_branch.take() {
            self.public_decode_state = super::decode::DecodeState::new(branch);
            true
        } else {
            self.public_op_busy_fault();
            self.public_decode_state = super::decode::DecodeState::empty();
            false
        }
    }

    #[inline]
    pub(in crate::endpoint) fn reset_public_decode_state(&mut self) {
        self.clear_session_waiter();
        self.finish_public_op(PublicActiveOp::Decode);
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
        self.clear_public_op_if_current(PublicActiveOp::Decode);
        let mut state = core::mem::replace(
            &mut self.public_decode_state,
            super::decode::DecodeState::empty(),
        );
        state.discard_terminal();
    }

    #[inline]
    pub(in crate::endpoint) fn revoke_clear_public_decode_state(&mut self) {
        self.clear_public_op_if_current(PublicActiveOp::Decode);
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
    pub(in crate::endpoint) fn primary_physical_lane(&self) -> Lane {
        self.port_for_lane(self.primary_lane).lane
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn register_session_waiter(&self, waker: &Waker) {
        let lane = self.primary_physical_lane();
        if let Some(cluster) = self.control.cluster() {
            cluster.register_session_waiter(self.public_rv, self.sid, lane, waker);
        }
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn clear_session_waiter(&self) {
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
}
