//! Observation helpers for endpoint tap emission and receive-frame acceptance.

use core::task::Poll;

use super::core::CursorEndpoint;
use crate::{
    endpoint::kernel::lane_port::{self, FrameMismatch},
    endpoint::{RecvError, RecvResult},
    observe::{events, ids},
    transport::{Transport, TransportError},
};

impl<'r, const ROLE: u8, T> CursorEndpoint<'r, ROLE, T>
where
    T: Transport + 'r,
{
    #[cold]
    #[inline]
    pub(in crate::endpoint::kernel) fn emit_materialization_mismatch_observation(
        &self,
        lane_idx: usize,
        lane_wire: u8,
        mismatch: FrameMismatch,
    ) {
        let port = self.port_for_lane(lane_idx);
        let event = mismatch.tap_event(port.now32(), self.sid.raw(), lane_wire);
        crate::observe::core::emit(port.tap(), event);
    }

    #[cold]
    #[inline]
    fn emit_materialized_transport_frame_observation(
        &self,
        lane_idx: usize,
        observation: lane_port::FrameObservation,
    ) {
        let port = self.port_for_lane(lane_idx);
        let event = crate::rendezvous::port::transport_frame_tap_event(port.now32(), observation);
        crate::observe::core::emit(port.tap(), event);
    }

    pub(in crate::endpoint::kernel) fn poll_received_transport_frame_for_lane(
        &mut self,
        pending_recv: &mut lane_port::PendingRecv,
        lane_idx: usize,
        lane_wire: u8,
        cx: &mut core::task::Context<'_>,
    ) -> Poll<RecvResult<lane_port::PreambleFrame<'r>>> {
        let port = self.port_for_lane(lane_idx);
        let transport_poll = lane_port::poll_recv_frame_preamble(
            pending_recv,
            port,
            self.sid.raw(),
            lane_wire,
            ROLE,
            cx,
        );
        if let Some(kind) = self.session_fault() {
            if let Poll::Ready(Ok(frame)) = transport_poll {
                frame.discard_uncommitted();
            }
            return Poll::Ready(Err(RecvError::SessionFault(kind)));
        }
        match transport_poll {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Ok(frame)) => Poll::Ready(Ok(frame)),
            Poll::Ready(Err(RecvError::Transport(err))) => {
                self.emit_transport_fault_event(lane_idx, lane_wire, err);
                Poll::Ready(Err(RecvError::Transport(err)))
            }
            Poll::Ready(Err(err)) => Poll::Ready(Err(err)),
        }
    }

    pub(in crate::endpoint::kernel) fn poll_received_framed_transport_frame_for_lane(
        &mut self,
        pending_recv: &mut lane_port::PendingRecv,
        lane_idx: usize,
        lane_wire: u8,
        cx: &mut core::task::Context<'_>,
    ) -> Poll<RecvResult<lane_port::PreambleFrame<'r>>> {
        match self.poll_received_transport_frame_for_lane(pending_recv, lane_idx, lane_wire, cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Ok(frame)) => {
                if frame.is_deterministic() {
                    self.emit_materialization_mismatch_observation(
                        lane_idx,
                        lane_wire,
                        lane_port::FrameMismatch::headerless_preamble(
                            self.sid.raw(),
                            lane_wire,
                            ROLE,
                        ),
                    );
                    frame.discard_uncommitted();
                    Poll::Ready(Err(RecvError::PhaseInvariant))
                } else {
                    Poll::Ready(Ok(frame))
                }
            }
            Poll::Ready(Err(err)) => Poll::Ready(Err(err)),
        }
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn poll_accepted_transport_frame(
        &mut self,
        pending_recv: &mut lane_port::PendingRecv,
        lane_idx: usize,
        expected: lane_port::FrameExpectation,
        cx: &mut core::task::Context<'_>,
    ) -> Poll<RecvResult<lane_port::ReceivedFrame<'r>>> {
        let port = self.port_for_lane(lane_idx);
        let transport_poll = lane_port::poll_recv_frame(pending_recv, port, expected, cx);
        if let Some(kind) = self.session_fault() {
            if let Poll::Ready(Ok(frame)) = transport_poll {
                frame.discard_uncommitted();
            }
            return Poll::Ready(Err(RecvError::SessionFault(kind)));
        }
        match transport_poll {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Ok(frame)) => Poll::Ready(Ok(frame)),
            Poll::Ready(Err(RecvError::Transport(err))) => {
                self.emit_transport_fault_event(lane_idx, expected.lane_wire, err);
                Poll::Ready(Err(RecvError::Transport(err)))
            }
            Poll::Ready(Err(err)) => Poll::Ready(Err(err)),
        }
    }

    pub(in crate::endpoint::kernel) fn accept_materialized_transport_frame(
        &self,
        lane_idx: usize,
        lane_wire: u8,
        source_role: u8,
        frame_label: u8,
        frame: lane_port::PreambleFrame<'r>,
    ) -> RecvResult<lane_port::ReceivedFrame<'r>> {
        let observed = match frame.preamble_observation(self.sid.raw(), lane_wire, ROLE) {
            lane_port::PreambleObservation::Framed(observation) => Some(observation),
            lane_port::PreambleObservation::Deterministic => None,
        };
        match frame.accept_parts(self.sid.raw(), ROLE, source_role, frame_label) {
            Ok(frame) => {
                if let Some(observed) = observed {
                    self.emit_materialized_transport_frame_observation(lane_idx, observed);
                }
                Ok(frame)
            }
            Err(mismatch) => {
                self.emit_materialization_mismatch_observation(lane_idx, lane_wire, mismatch);
                Err(RecvError::PhaseInvariant)
            }
        }
    }

    #[cold]
    #[inline(never)]
    pub(in crate::endpoint::kernel) fn emit_transport_fault_event(
        &self,
        lane_idx: usize,
        lane_wire: u8,
        error: TransportError,
    ) {
        let port = self.port_for_lane(lane_idx);
        let reason = transport_fault_reason(error);
        let event = events::raw_event(port.now32(), ids::TRANSPORT_FAULT)
            .with_causal_key(crate::observe::core::TapEvent::make_causal_key(
                lane_wire, reason,
            ))
            .with_arg0(self.sid.raw())
            .with_arg1(u32::from(lane_wire));
        crate::observe::core::emit(port.tap(), event);
    }
}

#[inline(always)]
const fn transport_fault_reason(error: TransportError) -> u8 {
    match error {
        TransportError::Offline => ids::TRANSPORT_FAULT_OFFLINE,
        TransportError::Deadline => ids::TRANSPORT_FAULT_DEADLINE,
        TransportError::Capacity => ids::TRANSPORT_FAULT_CAPACITY,
        TransportError::Failed => ids::TRANSPORT_FAULT_FAILED,
    }
}
