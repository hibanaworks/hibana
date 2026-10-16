//! Public endpoint polling entrypoints.

use core::task::Poll;

use super::{
    core::{
        OperationKernel, PublicActiveOp, PublicOpEdge, SendCommitOutcome, SendState,
        kernel_branch_recv, kernel_recv, kernel_send,
    },
    lane_port,
    offer::OfferState,
};
use crate::{
    endpoint::{RecvError, RecvResult, SendError, SendResult, carrier::WaiterTransfer},
    transport::{
        Transport,
        wire::{CodecError, Payload},
    },
};

impl<'r, T> OperationKernel<'_, 'r, T>
where
    T: Transport + 'r,
{
    #[inline]
    fn publish_public_route_branch(&mut self, label: u8) -> RecvResult<u8> {
        match self.transition_public_op(PublicOpEdge::PublishRouteBranch) {
            super::core::PublicOpLease::Held => Ok(label),
            super::core::PublicOpLease::Rejected | super::core::PublicOpLease::Faulted => Err(
                RecvError::SessionFault(crate::invariant_some(self.session_fault())),
            ),
        }
    }

    #[inline]
    pub(in crate::endpoint) fn poll_public_offer(
        &mut self,
        cx: &mut core::task::Context<'_>,
        waiters: &mut WaiterTransfer,
    ) -> Poll<RecvResult<u8>> {
        if let Some(kind) = self.session_fault() {
            self.clear_endpoint_waiter(waiters);
            self.fail_session(kind);
            return Poll::Ready(Err(RecvError::SessionFault(kind)));
        }
        if self.io.public_active_op != PublicActiveOp::Offer {
            self.public_op_busy_fault();
            let err = RecvError::PhaseInvariant;
            self.poison_for_recv_error(&err);
            return Poll::Ready(Err(err));
        }
        self.register_endpoint_waiter(waiters);
        if let Some(branch) = self.io.public_route_branch.as_ref() {
            let label = branch.branch_meta.label;
            self.clear_endpoint_waiter(waiters);
            self.io.public_offer_state = OfferState::new();
            return Poll::Ready(self.publish_public_route_branch(label));
        }
        let mut offer_state =
            core::mem::replace(&mut self.io.public_offer_state, OfferState::new());
        let poll = {
            let Some(mut scratch_lease) = self.context.session.rendezvous().try_scratch_lease()
            else {
                self.io.public_offer_state = offer_state;
                cx.waker().wake_by_ref();
                return Poll::Pending;
            };
            let mut frontier_scratch = self.frontier_scratch_workspace(&mut scratch_lease);
            self.poll_offer_state(&mut offer_state, cx, &mut frontier_scratch)
        };
        match poll {
            Poll::Pending => {
                self.io.public_offer_state = offer_state;
                Poll::Pending
            }
            Poll::Ready(Ok(label)) => {
                self.clear_endpoint_waiter(waiters);
                self.io.public_offer_state = OfferState::new();
                if self.io.public_route_branch.is_none() {
                    crate::invariant();
                }
                Poll::Ready(self.publish_public_route_branch(label))
            }
            Poll::Ready(Err(err)) => {
                offer_state.discard_terminal();
                self.clear_endpoint_waiter(waiters);
                let _ = self.transition_public_op(PublicOpEdge::FinishOffer);
                self.io.public_offer_state = OfferState::new();
                self.poison_for_recv_error(&err);
                Poll::Ready(Err(err))
            }
        }
    }

    #[inline]
    pub(in crate::endpoint) fn poll_public_recv(
        &mut self,
        logical_label: u8,
        payload_schema: u32,
        validate: for<'a> fn(Payload<'a>) -> Result<(), CodecError>,
        cx: &mut core::task::Context<'_>,
        waiters: &mut WaiterTransfer,
    ) -> Poll<RecvResult<Payload<'r>>> {
        if let Some(kind) = self.session_fault() {
            self.clear_endpoint_waiter(waiters);
            self.fail_session(kind);
            return Poll::Ready(Err(RecvError::SessionFault(kind)));
        }
        if self.io.public_active_op != PublicActiveOp::Recv {
            self.public_op_busy_fault();
            let err = RecvError::PhaseInvariant;
            self.poison_for_recv_error(&err);
            return Poll::Ready(Err(err));
        }
        self.register_endpoint_waiter(waiters);
        let mut recv_state = core::mem::replace(
            &mut self.io.public_recv_state,
            super::recv::RecvState::new(),
        );
        match kernel_recv(
            self,
            logical_label,
            payload_schema,
            validate,
            &mut recv_state,
            cx,
        ) {
            Poll::Pending => {
                self.io.public_recv_state = recv_state;
                Poll::Pending
            }
            Poll::Ready(result) => {
                self.clear_endpoint_waiter(waiters);
                let _ = self.transition_public_op(PublicOpEdge::FinishRecv);
                self.io.public_recv_state = super::recv::RecvState::new();
                match result {
                    Ok(payload) => Poll::Ready(Ok(payload)),
                    Err(err) => {
                        self.poison_for_recv_error(&err);
                        Poll::Ready(Err(err))
                    }
                }
            }
        }
    }

    #[inline]
    pub(in crate::endpoint) fn poll_public_branch_recv(
        &mut self,
        logical_label: u8,
        payload_schema: u32,
        validate: for<'a> fn(Payload<'a>) -> Result<(), crate::transport::wire::CodecError>,
        cx: &mut core::task::Context<'_>,
        waiters: &mut WaiterTransfer,
    ) -> Poll<RecvResult<Payload<'r>>> {
        if let Some(kind) = self.session_fault() {
            self.clear_endpoint_waiter(waiters);
            self.fail_session(kind);
            return Poll::Ready(Err(RecvError::SessionFault(kind)));
        }
        if self.io.public_active_op != PublicActiveOp::BranchRecv {
            self.public_op_busy_fault();
            let err = RecvError::PhaseInvariant;
            self.poison_for_recv_error(&err);
            return Poll::Ready(Err(err));
        }
        self.register_endpoint_waiter(waiters);
        let mut branch_recv_state = core::mem::replace(
            &mut self.io.public_branch_recv_state,
            super::branch_recv::BranchRecvState::empty(),
        );
        if self.io.public_route_branch.is_none() {
            self.clear_endpoint_waiter(waiters);
            self.io.public_branch_recv_state = super::branch_recv::BranchRecvState::empty();
            self.public_op_busy_fault();
            let err = RecvError::PhaseInvariant;
            return Poll::Ready(Err(err));
        }
        match kernel_branch_recv(
            self,
            logical_label,
            payload_schema,
            validate,
            &mut branch_recv_state,
            cx,
        ) {
            Poll::Pending => {
                self.io.public_branch_recv_state = branch_recv_state;
                Poll::Pending
            }
            Poll::Ready(result) => match result {
                Ok(payload) => {
                    self.clear_endpoint_waiter(waiters);
                    let _ = self.transition_public_op(PublicOpEdge::FinishBranchRecv);
                    self.io.public_branch_recv_state = super::branch_recv::BranchRecvState::empty();
                    let branch = crate::invariant_some(self.io.public_route_branch.take());
                    if branch.offered_frame.is_some() {
                        crate::invariant();
                    }
                    Poll::Ready(Ok(payload))
                }
                Err(err) => {
                    if let Some(branch) = self.io.public_route_branch.take() {
                        branch.discard_terminal();
                    }
                    self.clear_endpoint_waiter(waiters);
                    let _ = self.transition_public_op(PublicOpEdge::FinishBranchRecv);
                    self.io.public_branch_recv_state = super::branch_recv::BranchRecvState::empty();
                    self.poison_for_recv_error(&err);
                    Poll::Ready(Err(err))
                }
            },
        }
    }

    #[inline]
    pub(in crate::endpoint) fn poll_public_send(
        &mut self,
        cx: &mut core::task::Context<'_>,
        payload: Option<lane_port::RawSendPayload>,
        waiters: &mut WaiterTransfer,
    ) -> Poll<SendResult<SendCommitOutcome<'r>>> {
        let active_op = self.io.public_active_op;
        let branch_send = active_op == PublicActiveOp::BranchSend;
        if let Some(kind) = self.session_fault() {
            self.clear_endpoint_waiter(waiters);
            self.fail_session(kind);
            return Poll::Ready(Err(SendError::SessionFault(kind)));
        }
        if active_op != PublicActiveOp::Send && active_op != PublicActiveOp::BranchSend {
            self.public_op_busy_fault();
            let err = SendError::PhaseInvariant;
            self.poison_for_send_error(&err);
            return Poll::Ready(Err(err));
        }
        self.register_endpoint_waiter(waiters);
        let mut send_state = core::mem::replace(&mut self.io.public_send_state, SendState::Done);
        let mut payload = payload;
        let poll = kernel_send(self, &mut send_state, &mut payload, cx);
        match poll {
            Poll::Pending => {
                if payload.is_some() {
                    crate::invariant();
                }
                self.io.public_send_state = send_state;
                Poll::Pending
            }
            Poll::Ready(result) => {
                self.clear_endpoint_waiter(waiters);
                if branch_send {
                    let _ = self.transition_public_op(PublicOpEdge::FinishBranchSend);
                } else {
                    let _ = self.transition_public_op(PublicOpEdge::FinishSend);
                }
                self.io.public_send_state = SendState::Done;
                match result {
                    Ok(outcome) => {
                        if branch_send {
                            let branch = crate::invariant_some(self.io.public_route_branch.take());
                            if branch.offered_frame.is_some() {
                                crate::invariant();
                            }
                        }
                        Poll::Ready(Ok(outcome))
                    }
                    Err(err) => {
                        if branch_send && let Some(branch) = self.io.public_route_branch.take() {
                            branch.discard_terminal();
                        }
                        self.poison_for_send_error(&err);
                        Poll::Ready(Err(err))
                    }
                }
            }
        }
    }
}
