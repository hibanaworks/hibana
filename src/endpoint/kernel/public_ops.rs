//! Public endpoint operation lifecycle: preview reset, terminal clear, and waiter ownership.

use super::{
    core::{OperationKernel, PublicActiveOp, PublicOpEdge, SendState},
    lane_port,
    offer::OfferState,
};
use crate::{
    endpoint::{RecvError, SendError, carrier::WaiterTransfer},
    rendezvous::SessionFaultKind,
    transport::Transport,
};

impl<'r, T> OperationKernel<'_, 'r, T>
where
    T: Transport + 'r,
{
    #[inline]
    pub(in crate::endpoint::kernel) fn public_op_busy_fault(&mut self) {
        self.fail_session(SessionFaultKind::ProgressInvariantViolated);
    }

    #[inline]
    #[must_use]
    pub(in crate::endpoint::kernel) fn transition_public_op(
        &mut self,
        edge: PublicOpEdge,
    ) -> super::core::PublicOpLease {
        if matches!(
            edge,
            PublicOpEdge::BeginOffer | PublicOpEdge::BeginSend | PublicOpEdge::BeginRecv
        ) && self.has_other_active_operations()
        {
            self.public_op_busy_fault();
            return super::core::PublicOpLease::Rejected;
        }
        let transition = self.operation.public_active_op.transition(edge);
        self.operation.public_active_op = transition.phase();
        match transition.lease() {
            super::core::PublicOpLease::Held | super::core::PublicOpLease::Faulted => {}
            super::core::PublicOpLease::Rejected => {
                self.public_op_busy_fault();
            }
        }
        if self.operation.public_active_op != transition.phase() {
            crate::invariant();
        }
        transition.lease()
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn clear_public_op_if_current(&mut self, op: PublicActiveOp) {
        self.operation.public_active_op = self.operation.public_active_op.clear_if_current(op);
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn clear_public_op_terminal(&mut self) {
        self.operation.public_active_op = self.operation.public_active_op.clear_terminal();
    }

    #[inline]
    fn park_public_route_branch(&mut self, edge: PublicOpEdge) {
        if self.operation.public_route_branch.is_none() {
            self.public_op_busy_fault();
            return;
        }
        let _ = self.transition_public_op(edge);
    }

    #[inline]
    pub(in crate::endpoint) fn reset_public_offer_state(&mut self, waiters: &mut WaiterTransfer) {
        self.clear_endpoint_waiter(waiters);
        if self.operation.public_route_branch.is_some() {
            self.park_public_route_branch(PublicOpEdge::ParkOffer);
        } else {
            let _ = self.transition_public_op(PublicOpEdge::FinishOffer);
        }
        let mut state =
            core::mem::replace(&mut self.operation.public_offer_state, OfferState::new());
        self.restore_detached_offer_state(&mut state);
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn restore_detached_offer_state(
        &mut self,
        state: &mut OfferState<'r>,
    ) {
        let detached = state.take_detached_ingress();
        let mut requeue_failed = false;
        for payload in [
            detached.carried_transport_payload,
            detached.stage_transport_payload,
        ]
        .into_iter()
        .flatten()
        {
            let port = self.port_for_lane(payload.lane_idx());
            if payload.requeue_on(port).is_err() {
                requeue_failed = true;
            }
        }
        if requeue_failed {
            self.fail_session(SessionFaultKind::TransportClosed);
        }
    }

    #[inline]
    pub(in crate::endpoint) fn terminal_clear_public_offer_state(&mut self) {
        self.clear_public_op_if_current(PublicActiveOp::Offer);
        let mut state =
            core::mem::replace(&mut self.operation.public_offer_state, OfferState::new());
        state.discard_terminal();
        if let Some(branch) = self.operation.public_route_branch.take() {
            branch.discard_terminal();
        }
    }

    #[inline]
    #[must_use]
    pub(in crate::endpoint) fn init_public_offer_state(&mut self) -> super::core::PublicOpLease {
        let lease = match self.operation.public_active_op {
            PublicActiveOp::Idle if self.operation.public_route_branch.is_none() => {
                self.transition_public_op(PublicOpEdge::BeginOffer)
            }
            PublicActiveOp::RestoredRouteBranch if self.operation.public_route_branch.is_some() => {
                self.transition_public_op(PublicOpEdge::ResumeOffer)
            }
            PublicActiveOp::Idle | PublicActiveOp::RestoredRouteBranch => {
                self.public_op_busy_fault();
                super::core::PublicOpLease::Rejected
            }
            PublicActiveOp::Poisoned => super::core::PublicOpLease::Faulted,
            PublicActiveOp::Send
            | PublicActiveOp::Recv
            | PublicActiveOp::Offer
            | PublicActiveOp::RouteBranch
            | PublicActiveOp::BranchRecv
            | PublicActiveOp::BranchSend
            | PublicActiveOp::SendPayload => {
                self.public_op_busy_fault();
                super::core::PublicOpLease::Rejected
            }
        };
        match lease {
            super::core::PublicOpLease::Held => {}
            super::core::PublicOpLease::Rejected | super::core::PublicOpLease::Faulted => {
                return lease;
            }
        }
        self.operation.public_offer_state = OfferState::new();
        lease
    }

    #[inline]
    pub(in crate::endpoint) fn restore_public_route_branch(&mut self) {
        self.park_public_route_branch(PublicOpEdge::ParkRouteBranch);
    }

    #[inline]
    pub(in crate::endpoint) fn init_public_send_state(
        &mut self,
        logical_label: u8,
        payload_kind: crate::eff::EventPayload,
    ) -> crate::endpoint::SendResult<()> {
        if let Some(kind) = self.session_fault() {
            return Err(crate::endpoint::SendError::SessionFault(kind));
        }
        let lease = match self.operation.public_active_op {
            PublicActiveOp::Idle => self.transition_public_op(PublicOpEdge::BeginSend),
            PublicActiveOp::RouteBranch if self.operation.public_route_branch.is_some() => {
                self.transition_public_op(PublicOpEdge::BeginBranchSend)
            }
            PublicActiveOp::RouteBranch => {
                self.public_op_busy_fault();
                super::core::PublicOpLease::Rejected
            }
            PublicActiveOp::Poisoned => super::core::PublicOpLease::Faulted,
            PublicActiveOp::Send
            | PublicActiveOp::Recv
            | PublicActiveOp::Offer
            | PublicActiveOp::RestoredRouteBranch
            | PublicActiveOp::BranchRecv
            | PublicActiveOp::BranchSend
            | PublicActiveOp::SendPayload => {
                self.public_op_busy_fault();
                super::core::PublicOpLease::Rejected
            }
        };
        match lease {
            super::core::PublicOpLease::Held => {}
            super::core::PublicOpLease::Rejected => {
                return Err(crate::endpoint::SendError::PhaseInvariant);
            }
            super::core::PublicOpLease::Faulted => {
                return Err(crate::endpoint::SendError::SessionFault(
                    crate::invariant_some(self.session_fault()),
                ));
            }
        }
        self.operation.public_send_state = SendState::Preparing {
            logical_label,
            payload_kind,
            input: None,
        };
        Ok(())
    }

    #[inline]
    pub(in crate::endpoint) fn reset_public_send_state(&mut self, waiters: &mut WaiterTransfer) {
        self.clear_endpoint_waiter(waiters);
        match self.operation.public_active_op {
            PublicActiveOp::Send => {
                let _ = self.transition_public_op(PublicOpEdge::FinishSend);
            }
            PublicActiveOp::BranchSend => {
                self.park_public_route_branch(PublicOpEdge::ParkBranchSend);
            }
            PublicActiveOp::Poisoned => {}
            PublicActiveOp::Idle
            | PublicActiveOp::Recv
            | PublicActiveOp::Offer
            | PublicActiveOp::RouteBranch
            | PublicActiveOp::RestoredRouteBranch
            | PublicActiveOp::BranchRecv
            | PublicActiveOp::SendPayload => self.public_op_busy_fault(),
        }
        let state = core::mem::replace(&mut self.operation.public_send_state, SendState::Done);
        self.cancel_detached_send_state(state);
    }

    #[inline]
    pub(in crate::endpoint) fn terminal_clear_public_send_state(&mut self) {
        self.clear_public_op_if_current(PublicActiveOp::Send);
        self.clear_public_op_if_current(PublicActiveOp::BranchSend);
        let state = core::mem::replace(&mut self.operation.public_send_state, SendState::Done);
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
        let lease = self.transition_public_op(PublicOpEdge::BeginRecv);
        match lease {
            super::core::PublicOpLease::Held => {}
            super::core::PublicOpLease::Rejected | super::core::PublicOpLease::Faulted => {
                return lease;
            }
        }
        self.operation.public_recv_state = super::recv::RecvState::new();
        lease
    }

    #[inline]
    pub(in crate::endpoint) fn reset_public_recv_state(&mut self, waiters: &mut WaiterTransfer) {
        self.clear_endpoint_waiter(waiters);
        let _ = self.transition_public_op(PublicOpEdge::FinishRecv);
        self.operation.public_recv_state = super::recv::RecvState::new();
    }

    #[inline]
    pub(in crate::endpoint) fn terminal_clear_public_recv_state(&mut self) {
        self.clear_public_op_if_current(PublicActiveOp::Recv);
        self.operation.public_recv_state = super::recv::RecvState::new();
    }

    #[inline]
    #[must_use]
    pub(in crate::endpoint) fn begin_public_branch_recv_state(
        &mut self,
    ) -> super::core::PublicOpLease {
        let lease = self.transition_public_op(PublicOpEdge::BeginBranchRecv);
        match lease {
            super::core::PublicOpLease::Held => {}
            super::core::PublicOpLease::Rejected => {
                self.operation.public_branch_recv_state =
                    super::branch_recv::BranchRecvState::empty();
                return lease;
            }
            super::core::PublicOpLease::Faulted => return lease,
        }
        if self.operation.public_route_branch.is_none() {
            self.public_op_busy_fault();
            self.operation.public_branch_recv_state = super::branch_recv::BranchRecvState::empty();
            super::core::PublicOpLease::Rejected
        } else {
            self.operation.public_branch_recv_state = super::branch_recv::BranchRecvState::armed();
            lease
        }
    }

    #[inline]
    pub(in crate::endpoint) fn reset_public_branch_recv_state(
        &mut self,
        waiters: &mut WaiterTransfer,
    ) {
        self.clear_endpoint_waiter(waiters);
        if self.operation.public_active_op == PublicActiveOp::Poisoned {
            self.operation.public_branch_recv_state = super::branch_recv::BranchRecvState::empty();
            return;
        }
        self.park_public_route_branch(PublicOpEdge::ParkBranchRecv);
        self.operation.public_branch_recv_state = super::branch_recv::BranchRecvState::empty();
    }

    #[inline]
    pub(in crate::endpoint) fn terminal_clear_public_branch_recv_state(&mut self) {
        self.clear_public_op_if_current(PublicActiveOp::BranchRecv);
        if let Some(branch) = self.operation.public_route_branch.take() {
            branch.discard_terminal();
        }
        self.operation.public_branch_recv_state = super::branch_recv::BranchRecvState::empty();
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn session_fault(&self) -> Option<SessionFaultKind> {
        self.context.session.cluster().session_fault(
            self.context.public_rv,
            self.context.public_slot,
            self.context.public_generation.get(),
            self.context.sid,
        )
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn poison_session(
        &self,
        cause: SessionFaultKind,
    ) -> SessionFaultKind {
        self.context.session.cluster().poison_session(
            self.public_header.role(),
            self.context.public_rv,
            self.context.public_slot,
            self.context.public_generation.get(),
            self.context.sid,
            cause,
        )
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn retire_transport_handles(&mut self) {
        if self.io.ports.iter().all(Option::is_none) {
            return;
        }
        // A transport destructor is an external reentry point. Retirement is
        // only valid while this endpoint owns the rendezvous access barrier.
        let rendezvous = self.context.session.rendezvous();
        let retirement_access = match rendezvous.try_scratch_lease() {
            Some(access) => Some(access),
            None => {
                rendezvous.require_access_barrier();
                None
            }
        };
        for lane in 0..self.io.ports.len() {
            if self.owns_lane(lane)
                && let Some(port) = self.io.ports[lane].take()
            {
                drop(port);
            }
        }
        drop(retirement_access);
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn fail_session(
        &mut self,
        cause: SessionFaultKind,
    ) -> SessionFaultKind {
        let cause = self.poison_session(cause);
        self.terminal_clear_public_send_state();
        self.terminal_clear_public_recv_state();
        self.terminal_clear_public_offer_state();
        self.terminal_clear_public_branch_recv_state();
        if let Some(branch) = self.operation.public_route_branch.take() {
            branch.discard_terminal();
        }
        self.operation.public_active_op = self.operation.public_active_op.fault();
        self.retire_transport_handles();
        cause
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn register_endpoint_waiter(
        &self,
        waiters: &mut WaiterTransfer,
    ) {
        let replacement = waiters.take_replacement();
        let displaced = self.waiter.replace(replacement);
        waiters.defer(displaced);
    }

    #[inline]
    pub(crate) fn clear_endpoint_waiter(&self, waiters: &mut WaiterTransfer) {
        let waiter = self.waiter.take();
        waiters.defer(waiter);
    }

    #[inline]
    pub(in crate::endpoint) fn poison_for_recv_error(
        &mut self,
        error: &RecvError,
    ) -> SessionFaultKind {
        let cause = match error {
            RecvError::Transport(_) => SessionFaultKind::TransportClosed,
            RecvError::SessionFault(kind) => *kind,
            RecvError::Codec(_) => SessionFaultKind::DecodeFailed,
            RecvError::PhaseInvariant => SessionFaultKind::ProgressInvariantViolated,
            RecvError::LabelMismatch { .. }
            | RecvError::PayloadMismatch { .. }
            | RecvError::ResolverReject { .. } => SessionFaultKind::ProtocolViolation,
        };
        self.fail_session(cause)
    }

    #[inline]
    pub(in crate::endpoint) fn poison_for_send_error(
        &mut self,
        error: &SendError,
    ) -> SessionFaultKind {
        let cause = match error {
            SendError::Transport(_) => SessionFaultKind::TransportClosed,
            SendError::SessionFault(kind) => *kind,
            SendError::Codec(_)
            | SendError::LabelMismatch { .. }
            | SendError::PayloadMismatch { .. }
            | SendError::ResolverReject { .. } => SessionFaultKind::ProtocolViolation,
            SendError::PhaseInvariant => SessionFaultKind::ProgressInvariantViolated,
        };
        self.fail_session(cause)
    }
}
