//! Observation helpers for endpoint tap emission and receive-frame acceptance.

use core::task::Poll;

use super::core::CursorEndpoint;
use crate::{
    control::cap::mint::{EpochTable, MintConfigMarker},
    endpoint::kernel::lane_port::{self, FrameMismatch},
    endpoint::{RecvError, RecvResult},
    observe::{events::RawEvent, ids},
    runtime::{config::Clock, consts::LabelUniverse},
    transport::{Transport, TransportError},
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
    #[cold]
    #[inline]
    fn emit_materialization_mismatch_observation(
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

    #[inline(always)]
    pub(in crate::endpoint::kernel) fn poll_received_transport_frame_for_lane(
        &mut self,
        pending_recv: &mut lane_port::PendingRecv,
        lane_idx: usize,
        lane_wire: u8,
        cx: &mut core::task::Context<'_>,
    ) -> Poll<RecvResult<lane_port::PreambleFrame<'r>>> {
        let port = self.port_for_lane(lane_idx);
        match lane_port::poll_recv_frame_preamble(
            pending_recv,
            port,
            self.sid.raw(),
            lane_wire,
            ROLE,
            cx,
        ) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Ok(frame)) => Poll::Ready(Ok(frame)),
            Poll::Ready(Err(err)) => {
                self.emit_transport_fault_event(lane_idx, lane_wire, err);
                Poll::Ready(Err(RecvError::Transport(err)))
            }
        }
    }

    #[inline(always)]
    pub(in crate::endpoint::kernel) fn poll_accepted_transport_frame(
        &mut self,
        pending_recv: &mut lane_port::PendingRecv,
        lane_idx: usize,
        expected_session_raw: u32,
        expected_lane_wire: u8,
        expected_source_role: u8,
        expected_peer_role: u8,
        expected_label: u8,
        cx: &mut core::task::Context<'_>,
    ) -> Poll<RecvResult<lane_port::ReceivedFrame<'r>>> {
        let port = self.port_for_lane(lane_idx);
        match lane_port::poll_recv_frame(
            pending_recv,
            port,
            expected_session_raw,
            expected_lane_wire,
            expected_source_role,
            expected_peer_role,
            expected_label,
            cx,
        ) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Ok(frame)) => Poll::Ready(Ok(frame)),
            Poll::Ready(Err(err)) => {
                self.emit_transport_fault_event(lane_idx, expected_lane_wire, err);
                Poll::Ready(Err(RecvError::Transport(err)))
            }
        }
    }

    #[inline(always)]
    pub(in crate::endpoint::kernel) fn accept_materialized_transport_frame(
        &self,
        lane_idx: usize,
        lane_wire: u8,
        source_role: u8,
        frame_label: u8,
        frame: lane_port::PreambleFrame<'r>,
    ) -> Result<lane_port::ReceivedFrame<'r>, ()> {
        let observed = frame.observed_transport_frame(self.sid.raw(), lane_wire, ROLE);
        match frame.accept_parts(self.sid.raw(), ROLE, source_role, frame_label) {
            Ok(frame) => {
                if let Some(observation) = observed {
                    self.emit_materialized_transport_frame_observation(lane_idx, observation);
                }
                Ok(frame)
            }
            Err(mismatch) => {
                self.emit_materialization_mismatch_observation(lane_idx, lane_wire, mismatch);
                Err(())
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
        let event = RawEvent::new(port.now32(), ids::TRANSPORT_FAULT)
            .with_causal_key(crate::observe::core::TapEvent::make_causal_key(
                lane_wire, reason,
            ))
            .with_arg0(self.sid.raw());
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
