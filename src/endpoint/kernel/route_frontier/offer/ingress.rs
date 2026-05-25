//! Offer ingress collection and rollback ownership.

use core::task::Poll;

use super::{LaneIngressEvidence, OfferScopeSelection, OfferStagedIngress, RouteFrontierMachine};
use crate::{
    binding::BindingSlot,
    control::cap::mint::{EpochTable, MintConfigMarker},
    endpoint::{RecvError, RecvResult},
    runtime::{config::Clock, consts::LabelUniverse},
    transport::Transport,
};

use crate::endpoint::kernel::lane_port;

#[derive(Clone, Copy)]
pub(super) struct OfferFrontierFacts {
    pub(super) selection: OfferScopeSelection,
    pub(super) scope_id: crate::global::const_dsl::ScopeId,
    pub(super) offer_lane_idx: usize,
    pub(super) suppress_scope_frame_hint: bool,
    pub(super) is_route_controller: bool,
    pub(super) is_dynamic_route_scope: bool,
    pub(super) ingress_mode: OfferIngressMode,
}

#[derive(Clone, Copy)]
pub(super) enum OfferIngressMode {
    Skip,
    TransportOnly,
    ProbeSelectedBinding,
    ProbeSelectedAndRecvlessLoopBinding,
}

impl OfferIngressMode {
    #[inline]
    const fn probes_binding(self) -> bool {
        matches!(
            self,
            Self::ProbeSelectedBinding | Self::ProbeSelectedAndRecvlessLoopBinding
        )
    }

    #[inline]
    const fn recvless_loop_control_scope(self) -> bool {
        matches!(self, Self::ProbeSelectedAndRecvlessLoopBinding)
    }
}

pub(super) enum OfferIngressTurn<'a> {
    Binding(LaneIngressEvidence),
    Transport(lane_port::ReceivedFrame<'a>),
}

impl<'endpoint, 'r, const ROLE: u8, T, U, C, E, const MAX_RV: usize, Mint, B>
    RouteFrontierMachine<'endpoint, 'r, ROLE, T, U, C, E, MAX_RV, Mint, B>
where
    T: Transport + 'r,
    U: LabelUniverse,
    C: Clock,
    E: EpochTable,
    Mint: MintConfigMarker,
    B: BindingSlot + 'r,
{
    pub(super) fn await_transport_payload_for_offer_lane(
        &mut self,
        offer_lane: u8,
        ingress: &mut OfferStagedIngress<'r>,
        cx: &mut core::task::Context<'_>,
    ) -> Poll<RecvResult<()>> {
        assert!(
            !ingress.has_transport(),
            "offer transport wait must not poll while a received frame is already staged"
        );
        let lane_idx = offer_lane as usize;
        let port = self.endpoint.port_for_lane(lane_idx);
        let frame = match lane_port::poll_recv_frame(&mut self.pending_recv, lane_idx, port, cx) {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(Ok(frame)) => frame,
            Poll::Ready(Err(err)) => return Poll::Ready(Err(RecvError::Transport(err))),
        };
        ingress.stage_transport(frame);
        Poll::Ready(Ok(()))
    }

    #[inline]
    pub(super) fn requeue_offer_transport_payload(
        &mut self,
        payload: lane_port::ReceivedFrame<'r>,
    ) {
        let port = self.endpoint.port_for_lane(payload.lane_idx());
        lane_port::requeue_recv_frame(port, payload);
    }

    pub(super) fn collect_offer_ingress(
        &mut self,
        facts: OfferFrontierFacts,
        cx: &mut core::task::Context<'_>,
    ) -> Poll<RecvResult<Option<OfferIngressTurn<'r>>>> {
        if matches!(facts.ingress_mode, OfferIngressMode::Skip) {
            return Poll::Ready(Ok(None));
        }

        if let Some(evidence) = self.poll_offer_binding_ingress(facts) {
            return Poll::Ready(Ok(Some(OfferIngressTurn::Binding(evidence))));
        }

        let frame = {
            let port = self.endpoint.port_for_lane(facts.offer_lane_idx);
            match lane_port::poll_recv_frame(&mut self.pending_recv, facts.offer_lane_idx, port, cx)
            {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(Ok(frame)) => frame,
                Poll::Ready(Err(err)) => return Poll::Ready(Err(RecvError::Transport(err))),
            }
        };

        if let Some(evidence) = self.poll_offer_binding_ingress(facts) {
            self.requeue_offer_transport_payload(frame);
            return Poll::Ready(Ok(Some(OfferIngressTurn::Binding(evidence))));
        }

        Poll::Ready(Ok(Some(OfferIngressTurn::Transport(frame))))
    }

    fn poll_offer_binding_ingress(
        &mut self,
        facts: OfferFrontierFacts,
    ) -> Option<LaneIngressEvidence> {
        if !facts.ingress_mode.probes_binding() {
            return None;
        }

        let frame_label_meta = self.endpoint.selection_frame_label_meta(facts.selection);
        let materialization_meta = self
            .endpoint
            .selection_materialization_meta(facts.selection);
        if let Some((lane_idx, evidence)) = self.endpoint.poll_binding_for_offer(
            facts.scope_id,
            facts.offer_lane_idx,
            frame_label_meta,
            materialization_meta,
        ) {
            return Some(LaneIngressEvidence::new(lane_idx, evidence));
        }
        if facts.ingress_mode.recvless_loop_control_scope()
            && let Some((lane_idx, evidence)) = self.endpoint.poll_binding_any_for_offer(
                facts.offer_lane_idx,
                self.endpoint.offer_lane_set_for_scope(facts.scope_id),
            )
        {
            return Some(LaneIngressEvidence::new(lane_idx, evidence));
        }
        None
    }
}
