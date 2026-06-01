//! Offer ingress collection and rollback ownership.

use core::task::Poll;

use super::{
    CursorEndpoint, LaneIngressEvidence, OfferScopeProfile, OfferScopeSelection, OfferStagedIngress,
};
use crate::{
    binding::EndpointSlot,
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
    const fn probes_recvless_loop_binding(self) -> bool {
        matches!(self, Self::ProbeSelectedAndRecvlessLoopBinding)
    }
}

pub(super) enum OfferIngressTurn<'a> {
    Binding(LaneIngressEvidence),
    Transport(lane_port::ReceivedFrame<'a>),
}

impl<'r, const ROLE: u8, T, U, C, E, const MAX_RV: usize, Mint, B>
    CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>
where
    T: Transport + 'r,
    U: LabelUniverse,
    C: Clock,
    E: EpochTable,
    Mint: MintConfigMarker,
    B: EndpointSlot + 'r,
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
        let port = self.port_for_lane(offer_lane as usize);
        let frame = match lane_port::poll_recv_frame(pending_recv, port, cx) {
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
    ) -> RecvResult<()> {
        let port = self.port_for_lane(payload.lane_idx());
        lane_port::requeue_recv_frame(port, payload).map_err(RecvError::Transport)
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

        if let Some(evidence) = self.poll_offer_binding_ingress(facts) {
            return Poll::Ready(Ok(Some(OfferIngressTurn::Binding(evidence))));
        }

        let frame = {
            let port = self.port_for_lane(facts.offer_lane_idx());
            match lane_port::poll_recv_frame(pending_recv, port, cx) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(Ok(frame)) => frame,
                Poll::Ready(Err(err)) => return Poll::Ready(Err(RecvError::Transport(err))),
            }
        };

        if let Some(evidence) = self.poll_offer_binding_ingress(facts) {
            if let Err(err) = self.requeue_offer_transport_payload(frame) {
                return Poll::Ready(Err(err));
            }
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

        let frame_label_meta = self.selection_frame_label_meta(facts.selection);
        let materialization_meta = self.selection_materialization_meta(facts.selection);
        if let Some((lane_idx, evidence)) = self.poll_binding_for_offer(
            facts.scope_id(),
            facts.offer_lane_idx(),
            frame_label_meta,
            materialization_meta,
        ) {
            return Some(LaneIngressEvidence::new(lane_idx, evidence));
        }
        if facts.ingress_mode.probes_recvless_loop_binding()
            && let Some((lane_idx, evidence)) = self.poll_binding_any_for_offer(
                facts.offer_lane_idx(),
                self.offer_lane_set_for_scope(facts.scope_id()),
            )
        {
            return Some(LaneIngressEvidence::new(lane_idx, evidence));
        }
        None
    }
}
