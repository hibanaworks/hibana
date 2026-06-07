//! Offer ingress collection and rollback ownership.

use core::task::Poll;

use super::{CursorEndpoint, OfferScopeProfile, OfferScopeSelection, OfferStagedIngress};
use crate::{
    control::cap::mint::{EpochTable, MintConfigMarker},
    endpoint::{RecvError, RecvResult},
    runtime::{config::Clock, consts::LabelUniverse},
    transport::Transport,
};

use crate::endpoint::kernel::lane_port;

#[derive(Clone, Copy)]
pub(super) struct OfferFrontierFacts {
    pub(super) selection: OfferScopeSelection,
    pub(super) profile: OfferScopeProfile,
    pub(super) ingress_mode: OfferIngressMode,
}

impl OfferFrontierFacts {
    #[inline]
    pub(super) const fn scope_id(self) -> crate::global::const_dsl::ScopeId {
        self.selection.scope_id
    }

    #[inline]
    pub(super) const fn offer_lane_idx(self) -> usize {
        self.selection.offer_lane as usize
    }
}

#[derive(Clone, Copy)]
pub(super) enum OfferIngressMode {
    Skip,
    TransportOnly,
}

pub(super) enum OfferIngressTurn<'a> {
    Transport(lane_port::PreambleFrame<'a>),
}

impl<'r, const ROLE: u8, T, U, C, E, const MAX_RV: usize, Mint>
    CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint>
where
    T: Transport + 'r,
    U: LabelUniverse,
    C: Clock,
    E: EpochTable,
    Mint: MintConfigMarker,
{
    pub(super) fn await_transport_payload_for_offer_lane(
        &mut self,
        pending_recv: &mut lane_port::PendingRecv,
        offer_lane: u8,
        ingress: &mut OfferStagedIngress<'r>,
        cx: &mut core::task::Context<'_>,
    ) -> Poll<RecvResult<()>> {
        assert!(
            !ingress.has_transport(),
            "offer transport wait must not poll while a received frame is already staged"
        );
        let frame = match self.poll_received_transport_frame_for_lane(
            pending_recv,
            offer_lane as usize,
            offer_lane,
            cx,
        ) {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(Ok(frame)) => frame,
            Poll::Ready(Err(err)) => return Poll::Ready(Err(err)),
        };
        ingress.stage_transport(frame);
        Poll::Ready(Ok(()))
    }

    #[inline]
    pub(super) fn requeue_offer_transport_payload(
        &mut self,
        payload: lane_port::PreambleFrame<'r>,
    ) -> RecvResult<()> {
        let port = self.port_for_lane(payload.lane_idx());
        payload.requeue_on(port).map_err(RecvError::Transport)
    }

    pub(super) fn collect_offer_ingress(
        &mut self,
        pending_recv: &mut lane_port::PendingRecv,
        facts: OfferFrontierFacts,
        cx: &mut core::task::Context<'_>,
    ) -> Poll<RecvResult<Option<OfferIngressTurn<'r>>>> {
        if matches!(facts.ingress_mode, OfferIngressMode::Skip) {
            return Poll::Ready(Ok(None));
        }

        let frame = match self.poll_received_transport_frame_for_offer(pending_recv, facts, cx) {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(Ok(frame)) => frame,
            Poll::Ready(Err(err)) => return Poll::Ready(Err(err)),
        };

        Poll::Ready(Ok(Some(OfferIngressTurn::Transport(frame))))
    }

    fn poll_received_transport_frame_for_offer(
        &mut self,
        pending_recv: &mut lane_port::PendingRecv,
        facts: OfferFrontierFacts,
        cx: &mut core::task::Context<'_>,
    ) -> Poll<RecvResult<lane_port::PreambleFrame<'r>>> {
        let lane_limit = self.cursor.logical_lane_count();
        let lanes = self.transport_lane_set_for_offer(facts);
        let preferred_lane = self.preferred_transport_lane_for_offer(facts, lanes, lane_limit);
        let mut scan_idx = 0usize;
        while let Some(lane_idx) =
            next_preferred_transport_lane(preferred_lane, lanes, lane_limit, &mut scan_idx)
        {
            match self.poll_received_transport_frame_for_lane(
                pending_recv,
                lane_idx,
                lane_idx as u8,
                cx,
            ) {
                Poll::Pending => {}
                Poll::Ready(Ok(frame)) => return Poll::Ready(Ok(frame)),
                Poll::Ready(Err(err)) => return Poll::Ready(Err(err)),
            }
        }
        Poll::Pending
    }

    fn transport_lane_set_for_offer(
        &self,
        facts: OfferFrontierFacts,
    ) -> crate::global::role_program::LaneSetView<'static> {
        if let Some(token) = self.peek_scope_ack(facts.scope_id())
            && let Some(lanes) =
                self.route_scope_arm_lane_set_for_scope(facts.scope_id(), token.arm().as_u8())
        {
            return lanes;
        }
        self.offer_lane_set_for_scope(facts.scope_id())
    }

    fn preferred_transport_lane_for_offer(
        &self,
        facts: OfferFrontierFacts,
        lanes: crate::global::role_program::LaneSetView<'_>,
        lane_limit: usize,
    ) -> usize {
        if let Some((lane, _)) = self.peek_scope_frame_hint_with_lane(facts.scope_id()) {
            let lane_idx = lane as usize;
            if lane_idx < lane_limit && lanes.contains(lane_idx) {
                return lane_idx;
            }
        }
        if let Some(token) = self.peek_scope_ack(facts.scope_id())
            && let Some(arm_lanes) =
                self.route_scope_arm_lane_set_for_scope(facts.scope_id(), token.arm().as_u8())
            && let Some(lane_idx) = arm_lanes.first_set(lane_limit)
        {
            return lane_idx;
        }
        facts.offer_lane_idx()
    }
}

#[inline]
fn next_preferred_transport_lane(
    preferred_lane_idx: usize,
    offer_lanes: crate::global::role_program::LaneSetView<'_>,
    lane_limit: usize,
    scan_idx: &mut usize,
) -> Option<usize> {
    if *scan_idx == 0 {
        *scan_idx = 1;
        if preferred_lane_idx < lane_limit && offer_lanes.contains(preferred_lane_idx) {
            return Some(preferred_lane_idx);
        }
    }
    let mut candidate = offer_lanes.first_set(lane_limit);
    let mut skipped = 1usize;
    while skipped < *scan_idx {
        let lane_idx = candidate?;
        candidate = offer_lanes.next_set_from(lane_idx.saturating_add(1), lane_limit);
        skipped += 1;
    }
    while let Some(lane_idx) = candidate {
        *scan_idx += 1;
        if lane_idx != preferred_lane_idx {
            return Some(lane_idx);
        }
        candidate = offer_lanes.next_set_from(lane_idx.saturating_add(1), lane_limit);
    }
    None
}
