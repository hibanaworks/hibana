//! Public endpoint operation lifecycle: preview reset, terminal clear, and waiter ownership.

use core::task::Waker;

use super::{
    core::{CursorEndpoint, MaterializedRouteBranch, PublicActiveOp, SendInit, SendState},
    lane_port,
    offer::OfferState,
};
use crate::{
    endpoint::{RecvError, SendError},
    rendezvous::SessionFaultKind,
    session::types::Lane,
    transport::Transport,
};

impl<'r, const ROLE: u8, T> CursorEndpoint<'r, ROLE, T>
where
    T: Transport + 'r,
{
    #[inline]
    pub(in crate::endpoint::kernel) fn public_op_busy_fault(&mut self) {
        self.public_active_op = PublicActiveOp::Poisoned;
        self.poison_session(SessionFaultKind::ProgressInvariantViolated);
    }

    #[inline]
    #[must_use]
    pub(in crate::endpoint::kernel) fn start_public_op(
        &mut self,
        op: PublicActiveOp,
    ) -> super::core::PublicOpLease {
        if self.public_active_op == PublicActiveOp::Idle {
            self.public_active_op = op;
            super::core::PublicOpLease::Held
        } else {
            self.public_op_busy_fault();
            super::core::PublicOpLease::Rejected
        }
    }

    #[inline]
    #[must_use]
    fn transition_public_op(
        &mut self,
        from: PublicActiveOp,
        to: PublicActiveOp,
    ) -> super::core::PublicOpLease {
        if self.public_active_op == from {
            self.public_active_op = to;
            super::core::PublicOpLease::Held
        } else {
            self.public_op_busy_fault();
            super::core::PublicOpLease::Rejected
        }
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn finish_public_op(&mut self, op: PublicActiveOp) {
        if self.public_active_op == op {
            self.public_active_op = PublicActiveOp::Idle;
        } else if self.public_active_op != PublicActiveOp::Poisoned {
            self.public_op_busy_fault();
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
        branch: MaterializedRouteBranch<'r>,
    ) {
        if let Some(offered_frame) = branch.offered_frame {
            let frame = offered_frame.into_frame();
            let port = self.port_for_lane(frame.lane_idx());
            if lane_port::requeue_recv_frame(port, frame).is_err() {
                self.poison_session(SessionFaultKind::TransportClosed);
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
        let detached = state.take_detached_ingress();
        for payload in [
            detached.carried_transport_payload,
            detached.stage_transport_payload,
        ]
        .into_iter()
        .flatten()
        {
            let port = self.port_for_lane(payload.lane_idx());
            if payload.requeue_on(port).is_err() {
                self.poison_session(SessionFaultKind::TransportClosed);
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
    pub(in crate::endpoint) fn init_public_offer_state(&mut self) -> super::core::PublicOpLease {
        let lease = self.start_public_op(PublicActiveOp::Offer);
        match lease {
            super::core::PublicOpLease::Held => {}
            super::core::PublicOpLease::Rejected => return lease,
        }
        self.public_offer_state = OfferState::new();
        lease
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
    pub(in crate::endpoint) fn init_public_send_state(
        &mut self,
        init: &SendInit,
    ) -> super::core::PublicOpLease {
        let lease = match self.public_active_op {
            PublicActiveOp::Idle => self.start_public_op(PublicActiveOp::Send),
            PublicActiveOp::RouteBranch if self.public_route_branch.is_some() => {
                self.transition_public_op(PublicActiveOp::RouteBranch, PublicActiveOp::BranchSend)
            }
            PublicActiveOp::RouteBranch => {
                self.public_op_busy_fault();
                super::core::PublicOpLease::Rejected
            }
            _ => {
                self.public_op_busy_fault();
                super::core::PublicOpLease::Rejected
            }
        };
        match lease {
            super::core::PublicOpLease::Held => {}
            super::core::PublicOpLease::Rejected => return lease,
        }
        let (meta, preview_cursor_index, resolver_decisions) = init.preview.into_parts();
        self.public_send_state = SendState::Init {
            descriptor: init.descriptor,
            meta,
            preview_cursor_index: Some(preview_cursor_index),
            resolver_decisions,
        };
        lease
    }

    #[inline]
    pub(in crate::endpoint) fn reset_public_send_state(&mut self) {
        self.clear_session_waiter();
        let restore_branch = self.public_active_op == PublicActiveOp::BranchSend;
        match self.public_active_op {
            PublicActiveOp::Send => self.finish_public_op(PublicActiveOp::Send),
            PublicActiveOp::BranchSend => self.finish_public_op(PublicActiveOp::BranchSend),
            PublicActiveOp::Poisoned => {}
            _ => self.public_op_busy_fault(),
        }
        let state = core::mem::replace(&mut self.public_send_state, SendState::Done);
        self.cancel_detached_send_state(state);
        if restore_branch {
            if let Some(branch) = self.public_route_branch.take() {
                self.restore_materialized_route_branch(branch);
            } else {
                self.public_op_busy_fault();
            }
        }
    }

    #[inline]
    pub(in crate::endpoint) fn terminal_clear_public_send_state(&mut self) {
        self.clear_session_waiter();
        self.clear_public_op_if_current(PublicActiveOp::Send);
        self.clear_public_op_if_current(PublicActiveOp::BranchSend);
        let state = core::mem::replace(&mut self.public_send_state, SendState::Done);
        self.cancel_detached_send_state(state);
    }

    #[inline]
    fn cancel_detached_send_state(&mut self, state: SendState<'r>) {
        if let SendState::Sending { mut pending, .. } = state {
            let lane_idx = pending.lane_idx();
            pending.commit_plan = None;
            let port = self.port_for_lane(lane_idx);
            lane_port::cancel_send_outgoing(&mut pending.transport, port);
        }
    }

    #[inline]
    #[must_use]
    pub(in crate::endpoint) fn init_public_recv_state(&mut self) -> super::core::PublicOpLease {
        let lease = self.start_public_op(PublicActiveOp::Recv);
        match lease {
            super::core::PublicOpLease::Held => {}
            super::core::PublicOpLease::Rejected => return lease,
        }
        self.public_recv_state = super::recv::RecvState::new();
        lease
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
    #[must_use]
    pub(in crate::endpoint) fn begin_public_branch_recv_state(
        &mut self,
    ) -> super::core::PublicOpLease {
        let lease =
            self.transition_public_op(PublicActiveOp::RouteBranch, PublicActiveOp::BranchRecv);
        match lease {
            super::core::PublicOpLease::Held => {}
            super::core::PublicOpLease::Rejected => {
                self.public_branch_recv_state = super::branch_recv::BranchRecvState::empty();
                return lease;
            }
        }
        if let Some(branch) = self.public_route_branch.take() {
            self.public_branch_recv_state = super::branch_recv::BranchRecvState::new(branch);
            lease
        } else {
            self.public_op_busy_fault();
            self.public_branch_recv_state = super::branch_recv::BranchRecvState::empty();
            super::core::PublicOpLease::Rejected
        }
    }

    #[inline]
    pub(in crate::endpoint) fn reset_public_branch_recv_state(&mut self) {
        self.clear_session_waiter();
        self.finish_public_op(PublicActiveOp::BranchRecv);
        if self.public_branch_recv_state.restore_on_drop()
            == super::branch_recv::BranchRecvRestore::Armed
            && let Some(branch) = self.public_branch_recv_state.branch.take()
        {
            self.restore_materialized_route_branch(branch);
        }
        self.public_branch_recv_state = super::branch_recv::BranchRecvState::empty();
    }

    #[inline]
    pub(in crate::endpoint) fn terminal_clear_public_branch_recv_state(&mut self) {
        self.clear_session_waiter();
        self.clear_public_op_if_current(PublicActiveOp::BranchRecv);
        let mut state = core::mem::replace(
            &mut self.public_branch_recv_state,
            super::branch_recv::BranchRecvState::empty(),
        );
        state.discard_terminal();
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn session_fault(&self) -> Option<SessionFaultKind> {
        self.session
            .cluster()
            .session_fault(self.public_rv, self.sid)
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn poison_session(
        &self,
        cause: SessionFaultKind,
    ) -> SessionFaultKind {
        self.session
            .cluster()
            .poison_session(self.public_rv, self.sid, cause)
    }

    #[inline]
    pub(in crate::endpoint) fn primary_physical_lane(&self) -> Lane {
        self.port_for_lane(self.primary_lane).lane
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn register_session_waiter(&self, waker: &Waker) {
        let lane = self.primary_physical_lane();
        self.session
            .cluster()
            .register_session_waiter(self.public_rv, self.sid, lane, waker);
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn clear_session_waiter(&self) {
        let lane = self.primary_physical_lane();
        self.session
            .cluster()
            .clear_session_waiter(self.public_rv, self.sid, lane);
    }

    #[inline]
    pub(in crate::endpoint) fn poison_for_recv_error(&self, error: &RecvError) -> SessionFaultKind {
        let cause = match error {
            RecvError::Transport(_) => SessionFaultKind::TransportClosed,
            RecvError::SessionFault(kind) => *kind,
            RecvError::Codec(_) => SessionFaultKind::DecodeFailed,
            RecvError::PhaseInvariant => SessionFaultKind::ProgressInvariantViolated,
            RecvError::LabelMismatch { .. } | RecvError::ResolverReject { .. } => {
                SessionFaultKind::ProtocolViolation
            }
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
            | SendError::ResolverReject { .. } => SessionFaultKind::ProtocolViolation,
            SendError::PhaseInvariant => SessionFaultKind::ProgressInvariantViolated,
        };
        self.poison_session(cause)
    }
}
