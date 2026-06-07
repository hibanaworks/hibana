//! Public endpoint polling entrypoints.

use core::task::Poll;

use super::{
    core::{
        CursorEndpoint, DecodeRuntimeDesc, PublicActiveOp, SendCommitOutcome, SendState,
        kernel_decode, kernel_recv, kernel_send,
    },
    lane_port,
    offer::OfferState,
};
use crate::{
    control::cap::mint::{EpochTable, MintConfigMarker},
    endpoint::{RecvError, RecvResult, SendError, SendResult},
    global::ControlDesc,
    runtime::{config::Clock, consts::LabelUniverse},
    transport::{
        Transport,
        wire::{CodecError, Payload},
    },
};

impl<'r, const ROLE: u8, T, U, C, E, const MAX_RV: usize, Mint>
    CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint>
where
    T: Transport + 'r,
    U: LabelUniverse,
    C: Clock,
    E: EpochTable,
    Mint: MintConfigMarker,
{
    #[inline]
    pub(in crate::endpoint) fn poll_public_offer(
        &mut self,
        cx: &mut core::task::Context<'_>,
    ) -> Poll<RecvResult<u8>> {
        if self.public_active_op != PublicActiveOp::Offer {
            self.public_op_busy_fault();
            let err = RecvError::PhaseInvariant;
            let _ = self.poison_for_recv_error(&err);
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
                    self.public_active_op = PublicActiveOp::RouteBranch;
                    Poll::Ready(Ok(label))
                }
            }
            Poll::Ready(Err(err)) => {
                offer_state.discard_terminal();
                self.clear_session_waiter();
                self.finish_public_op(PublicActiveOp::Offer);
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
        control: Option<ControlDesc>,
        accepts_empty_payload: bool,
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
            let _ = self.poison_for_recv_error(&err);
            return Poll::Ready(Err(err));
        }
        let mut recv_state =
            core::mem::replace(&mut self.public_recv_state, super::recv::RecvState::new());
        match kernel_recv(
            self,
            logical_label,
            expects_control,
            control,
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
                self.finish_public_op(PublicActiveOp::Recv);
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
        control: Option<ControlDesc>,
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
        if self.public_active_op != PublicActiveOp::Decode {
            self.public_op_busy_fault();
            let err = RecvError::PhaseInvariant;
            let _ = self.poison_for_recv_error(&err);
            return Poll::Ready(Err(err));
        }
        let mut decode_state = core::mem::replace(
            &mut self.public_decode_state,
            super::decode::DecodeState::empty(),
        );
        let Some(branch) = decode_state.branch() else {
            self.clear_session_waiter();
            self.public_decode_state = super::decode::DecodeState::empty();
            self.public_op_busy_fault();
            let err = RecvError::PhaseInvariant;
            return Poll::Ready(Err(err));
        };
        let descriptor = DecodeRuntimeDesc::new(
            logical_label,
            crate::transport::FrameLabel::new(branch.branch_meta.frame_label),
            expects_control,
            validate,
            synthetic,
        );
        match kernel_decode(self, descriptor, control, &mut decode_state, cx) {
            Poll::Pending => {
                self.register_session_waiter(cx.waker());
                self.public_decode_state = decode_state;
                Poll::Pending
            }
            Poll::Ready(result) => match result {
                Ok(payload) => {
                    self.clear_session_waiter();
                    self.finish_public_op(PublicActiveOp::Decode);
                    self.public_decode_state = super::decode::DecodeState::empty();
                    Poll::Ready(Ok(payload))
                }
                Err(err) => {
                    decode_state.discard_terminal();
                    self.clear_session_waiter();
                    self.finish_public_op(PublicActiveOp::Decode);
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
        payload: Option<lane_port::RawSendPayload>,
    ) -> Poll<SendResult<SendCommitOutcome<'r>>>
    where
        <Mint as MintConfigMarker>::Policy: crate::control::cap::mint::AllowsEndpointMint,
    {
        if let Some(kind) = self.session_fault() {
            self.reset_public_send_state();
            return Poll::Ready(Err(SendError::SessionFault(kind)));
        }
        if self.public_active_op != PublicActiveOp::Send {
            self.public_op_busy_fault();
            let err = SendError::PhaseInvariant;
            let _ = self.poison_for_send_error(&err);
            return Poll::Ready(Err(err));
        }
        let mut send_state = core::mem::replace(&mut self.public_send_state, SendState::Done);
        let mut payload = payload;
        match kernel_send(self, &mut send_state, &mut payload, cx) {
            Poll::Pending => {
                debug_assert!(
                    payload.is_none(),
                    "send init payload must be consumed before pending transport"
                );
                self.register_session_waiter(cx.waker());
                self.public_send_state = send_state;
                Poll::Pending
            }
            Poll::Ready(result) => {
                self.clear_session_waiter();
                self.finish_public_op(PublicActiveOp::Send);
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
