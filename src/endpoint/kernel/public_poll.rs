//! Public endpoint polling entrypoints.

use core::task::Poll;

use super::{
    core::{
        CursorEndpoint, PublicActiveOp, SendCommitOutcome, SendState, kernel_branch_recv,
        kernel_recv, kernel_send,
    },
    lane_port,
    offer::OfferState,
};
use crate::{
    endpoint::{RecvError, RecvResult, SendError, SendResult},
    transport::{
        Transport,
        wire::{CodecError, Payload},
    },
};

impl<'r, const ROLE: u8, T> CursorEndpoint<'r, ROLE, T>
where
    T: Transport + 'r,
{
    #[inline]
    pub(in crate::endpoint) fn poll_public_offer(
        &mut self,
        cx: &mut core::task::Context<'_>,
    ) -> Poll<RecvResult<u8>> {
        if self.public_active_op != PublicActiveOp::Offer {
            self.public_op_busy_fault();
            let err = RecvError::PhaseInvariant;
            self.poison_for_recv_error(&err);
            return Poll::Ready(Err(err));
        }
        if let Some(kind) = self.session_fault() {
            self.terminal_clear_public_offer_state();
            return Poll::Ready(Err(RecvError::SessionFault(kind)));
        }
        if self.public_route_branch.is_some() {
            self.clear_session_waiter();
            self.public_op_busy_fault();
            let err = RecvError::PhaseInvariant;
            return Poll::Ready(Err(err));
        }
        let mut offer_state = core::mem::replace(&mut self.public_offer_state, OfferState::new());
        let poll = self.poll_offer_state(&mut offer_state, cx);
        match poll {
            Poll::Pending => {
                self.register_session_waiter(cx.waker());
                self.public_offer_state = offer_state;
                Poll::Pending
            }
            Poll::Ready(Ok(label)) => {
                self.clear_session_waiter();
                self.public_offer_state = OfferState::new();
                if self.public_route_branch.is_none() {
                    crate::invariant();
                }
                self.public_active_op = PublicActiveOp::RouteBranch;
                Poll::Ready(Ok(label))
            }
            Poll::Ready(Err(err)) => {
                offer_state.discard_terminal();
                self.clear_session_waiter();
                self.finish_public_op(PublicActiveOp::Offer);
                self.public_offer_state = OfferState::new();
                self.poison_for_recv_error(&err);
                Poll::Ready(Err(err))
            }
        }
    }

    #[inline]
    pub(in crate::endpoint) fn poll_public_recv(
        &mut self,
        logical_label: u8,
        validate: for<'a> fn(Payload<'a>) -> Result<(), CodecError>,
        cx: &mut core::task::Context<'_>,
    ) -> Poll<RecvResult<Payload<'r>>> {
        if let Some(kind) = self.session_fault() {
            self.terminal_clear_public_recv_state();
            return Poll::Ready(Err(RecvError::SessionFault(kind)));
        }
        if self.public_active_op != PublicActiveOp::Recv {
            self.public_op_busy_fault();
            let err = RecvError::PhaseInvariant;
            self.poison_for_recv_error(&err);
            return Poll::Ready(Err(err));
        }
        let mut recv_state =
            core::mem::replace(&mut self.public_recv_state, super::recv::RecvState::new());
        match kernel_recv(self, logical_label, validate, &mut recv_state, cx) {
            Poll::Pending => {
                self.register_session_waiter(cx.waker());
                self.public_recv_state = recv_state;
                Poll::Pending
            }
            Poll::Ready(result) => {
                self.clear_session_waiter();
                self.finish_public_op(PublicActiveOp::Recv);
                self.public_recv_state = super::recv::RecvState::new();
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
        validate: for<'a> fn(Payload<'a>) -> Result<(), crate::transport::wire::CodecError>,
        cx: &mut core::task::Context<'_>,
    ) -> Poll<RecvResult<Payload<'r>>> {
        if let Some(kind) = self.session_fault() {
            self.terminal_clear_public_branch_recv_state();
            return Poll::Ready(Err(RecvError::SessionFault(kind)));
        }
        if self.public_active_op != PublicActiveOp::BranchRecv {
            self.public_op_busy_fault();
            let err = RecvError::PhaseInvariant;
            self.poison_for_recv_error(&err);
            return Poll::Ready(Err(err));
        }
        let mut branch_recv_state = core::mem::replace(
            &mut self.public_branch_recv_state,
            super::branch_recv::BranchRecvState::empty(),
        );
        if self.public_route_branch.is_none() {
            self.clear_session_waiter();
            self.public_branch_recv_state = super::branch_recv::BranchRecvState::empty();
            self.public_op_busy_fault();
            let err = RecvError::PhaseInvariant;
            return Poll::Ready(Err(err));
        }
        match kernel_branch_recv(self, logical_label, validate, &mut branch_recv_state, cx) {
            Poll::Pending => {
                self.register_session_waiter(cx.waker());
                self.public_branch_recv_state = branch_recv_state;
                Poll::Pending
            }
            Poll::Ready(result) => match result {
                Ok(payload) => {
                    self.clear_session_waiter();
                    self.finish_public_op(PublicActiveOp::BranchRecv);
                    self.public_branch_recv_state = super::branch_recv::BranchRecvState::empty();
                    let branch = crate::invariant_some(self.public_route_branch.take());
                    if branch.offered_frame.is_some() {
                        crate::invariant();
                    }
                    Poll::Ready(Ok(payload))
                }
                Err(err) => {
                    if let Some(branch) = self.public_route_branch.take() {
                        branch.discard_terminal();
                    }
                    self.clear_session_waiter();
                    self.finish_public_op(PublicActiveOp::BranchRecv);
                    self.public_branch_recv_state = super::branch_recv::BranchRecvState::empty();
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
    ) -> Poll<SendResult<SendCommitOutcome<'r>>> {
        let active_op = self.public_active_op;
        let branch_send = active_op == PublicActiveOp::BranchSend;
        if let Some(kind) = self.session_fault() {
            if branch_send {
                self.terminal_clear_public_send_state();
                if let Some(branch) = self.public_route_branch.take() {
                    branch.discard_terminal();
                }
            } else {
                self.reset_public_send_state();
            }
            return Poll::Ready(Err(SendError::SessionFault(kind)));
        }
        if active_op != PublicActiveOp::Send && active_op != PublicActiveOp::BranchSend {
            self.public_op_busy_fault();
            let err = SendError::PhaseInvariant;
            self.poison_for_send_error(&err);
            return Poll::Ready(Err(err));
        }
        let mut send_state = core::mem::replace(&mut self.public_send_state, SendState::Done);
        let mut payload = payload;
        match kernel_send(self, &mut send_state, &mut payload, cx) {
            Poll::Pending => {
                if payload.is_some() {
                    crate::invariant();
                }
                self.register_session_waiter(cx.waker());
                self.public_send_state = send_state;
                Poll::Pending
            }
            Poll::Ready(result) => {
                self.clear_session_waiter();
                if branch_send {
                    self.finish_public_op(PublicActiveOp::BranchSend);
                } else {
                    self.finish_public_op(PublicActiveOp::Send);
                }
                self.public_send_state = SendState::Done;
                match result {
                    Ok(outcome) => {
                        if branch_send {
                            let branch = crate::invariant_some(self.public_route_branch.take());
                            if branch.offered_frame.is_some() {
                                crate::invariant();
                            }
                        }
                        Poll::Ready(Ok(outcome))
                    }
                    Err(err) => {
                        if branch_send && let Some(branch) = self.public_route_branch.take() {
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
