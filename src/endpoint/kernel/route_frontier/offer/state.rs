//! Offer preview state and rollback ownership.

use super::ingress::OfferFrontierFacts;
#[cfg(test)]
use super::ingress::OfferIngressMode;
use super::{LaneIngressEvidence, OfferProgressState, OfferScopeSelection, ResolvePendingAction};
use crate::endpoint::kernel::lane_port;

#[cfg(test)]
use crate::transport::wire::Payload;

pub(super) struct OfferCollectState<'a> {
    pub(super) selection: OfferScopeSelection,
    pub(super) facts: OfferFrontierFacts,
    pub(super) binding_evidence: Option<LaneIngressEvidence>,
    pub(super) transport_payload: Option<lane_port::ReceivedFrame<'a>>,
}

impl OfferCollectState<'_> {
    #[inline]
    pub(super) fn discard_terminal(&mut self) {
        if let Some(payload) = self.transport_payload.take() {
            payload.discard_uncommitted();
        }
    }
}

pub(super) struct OfferResolveState<'a> {
    pub(super) selection: OfferScopeSelection,
    pub(super) facts: OfferFrontierFacts,
    pub(super) binding_evidence: Option<LaneIngressEvidence>,
    pub(super) transport_payload: Option<lane_port::ReceivedFrame<'a>>,
    pub(super) progress: OfferProgressState,
    pub(super) pending_action: Option<ResolvePendingAction>,
    pub(super) yield_armed: bool,
}

impl OfferResolveState<'_> {
    #[inline]
    pub(super) fn discard_terminal(&mut self) {
        if let Some(payload) = self.transport_payload.take() {
            payload.discard_uncommitted();
        }
    }
}

pub(super) enum OfferRunStage<'a> {
    CollectEvidence(OfferCollectState<'a>),
    ResolveToken(OfferResolveState<'a>),
}

impl OfferRunStage<'_> {
    #[inline]
    pub(super) fn discard_terminal(&mut self) {
        match self {
            Self::CollectEvidence(stage) => stage.discard_terminal(),
            Self::ResolveToken(stage) => stage.discard_terminal(),
        }
    }
}

pub(in crate::endpoint::kernel) struct OfferRollbackItems<'r> {
    pub(in crate::endpoint::kernel) carried_binding_evidence: Option<LaneIngressEvidence>,
    pub(in crate::endpoint::kernel) carried_transport_payload: Option<lane_port::ReceivedFrame<'r>>,
    pub(in crate::endpoint::kernel) stage_binding_evidence: Option<LaneIngressEvidence>,
    pub(in crate::endpoint::kernel) stage_transport_payload: Option<lane_port::ReceivedFrame<'r>>,
}

impl OfferRollbackItems<'_> {
    #[inline]
    pub(in crate::endpoint::kernel) fn discard_terminal(&mut self) {
        if let Some(payload) = self.carried_transport_payload.take() {
            payload.discard_uncommitted();
        }
        if let Some(payload) = self.stage_transport_payload.take() {
            payload.discard_uncommitted();
        }
    }
}

pub(crate) struct OfferState<'r> {
    frontier_visited: Option<super::FrontierVisitSet>,
    pub(super) carried_binding_evidence: Option<LaneIngressEvidence>,
    pub(super) carried_transport_payload: Option<lane_port::ReceivedFrame<'r>>,
    pub(super) run_stage: Option<OfferRunStage<'r>>,
    pub(super) pending_recv: lane_port::PendingRecv,
    pub(crate) deadline: crate::endpoint::kernel::core::WaitDeadline,
}

impl<'r> OfferState<'r> {
    #[inline]
    pub(crate) const fn new() -> Self {
        Self {
            frontier_visited: None,
            carried_binding_evidence: None,
            carried_transport_payload: None,
            run_stage: None,
            pending_recv: lane_port::PendingRecv::new(),
            deadline: crate::endpoint::kernel::core::WaitDeadline::new(),
        }
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn take_rollback_items(&mut self) -> OfferRollbackItems<'r> {
        let mut items = OfferRollbackItems {
            carried_binding_evidence: self.carried_binding_evidence.take(),
            carried_transport_payload: self.carried_transport_payload.take(),
            stage_binding_evidence: None,
            stage_transport_payload: None,
        };
        if let Some(stage) = self.run_stage.take() {
            match stage {
                OfferRunStage::CollectEvidence(stage) => {
                    items.stage_binding_evidence = stage.binding_evidence;
                    items.stage_transport_payload = stage.transport_payload;
                }
                OfferRunStage::ResolveToken(stage) => {
                    items.stage_binding_evidence = stage.binding_evidence;
                    items.stage_transport_payload = stage.transport_payload;
                }
            }
        }
        items
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn discard_terminal(&mut self) {
        let mut rollback = self.take_rollback_items();
        rollback.discard_terminal();
    }

    #[cfg(test)]
    pub(in crate::endpoint::kernel) fn stage_carried_binding_evidence_for_test(
        &mut self,
        evidence: LaneIngressEvidence,
    ) {
        self.carried_binding_evidence = Some(evidence);
    }

    #[cfg(test)]
    pub(in crate::endpoint::kernel) fn stage_carried_transport_payload_for_test(
        &mut self,
        lane: u8,
        payload: Payload<'r>,
    ) {
        self.carried_transport_payload = Some(lane_port::ReceivedFrame::synthetic_for_test(
            lane as usize,
            payload,
        ));
    }

    #[cfg(test)]
    pub(in crate::endpoint::kernel) fn stage_collect_rollback_items_for_test(
        &mut self,
        binding_evidence: Option<LaneIngressEvidence>,
        transport_payload: Option<(u8, Payload<'r>)>,
    ) {
        let selection = offer_rollback_selection_for_test();
        self.run_stage = Some(OfferRunStage::CollectEvidence(OfferCollectState {
            selection,
            facts: offer_rollback_facts_for_test(selection),
            binding_evidence,
            transport_payload: transport_payload.map(|(lane, payload)| {
                lane_port::ReceivedFrame::synthetic_for_test(lane as usize, payload)
            }),
        }));
    }

    #[cfg(test)]
    pub(in crate::endpoint::kernel) fn stage_resolve_rollback_items_for_test(
        &mut self,
        binding_evidence: Option<LaneIngressEvidence>,
        transport_payload: Option<(u8, Payload<'r>)>,
    ) {
        let selection = offer_rollback_selection_for_test();
        self.run_stage = Some(OfferRunStage::ResolveToken(OfferResolveState {
            selection,
            facts: offer_rollback_facts_for_test(selection),
            binding_evidence,
            transport_payload: transport_payload.map(|(lane, payload)| {
                lane_port::ReceivedFrame::synthetic_for_test(lane as usize, payload)
            }),
            progress: OfferProgressState::new(crate::runtime::config::OfferProgressPolicy),
            pending_action: None,
            yield_armed: false,
        }));
    }

    #[inline]
    pub(super) fn into_machine_parts(
        &mut self,
    ) -> (
        Option<super::FrontierVisitSet>,
        Option<LaneIngressEvidence>,
        Option<lane_port::ReceivedFrame<'r>>,
        Option<OfferRunStage<'r>>,
        lane_port::PendingRecv,
    ) {
        (
            self.frontier_visited.take(),
            self.carried_binding_evidence.take(),
            self.carried_transport_payload.take(),
            self.run_stage.take(),
            core::mem::replace(&mut self.pending_recv, lane_port::PendingRecv::new()),
        )
    }

    #[inline]
    pub(super) fn store_machine_parts(
        &mut self,
        frontier_visited: Option<super::FrontierVisitSet>,
        carried_binding_evidence: Option<LaneIngressEvidence>,
        carried_transport_payload: Option<lane_port::ReceivedFrame<'r>>,
        run_stage: Option<OfferRunStage<'r>>,
        pending_recv: lane_port::PendingRecv,
    ) {
        self.frontier_visited = frontier_visited;
        self.carried_binding_evidence = carried_binding_evidence;
        self.carried_transport_payload = carried_transport_payload;
        self.run_stage = run_stage;
        self.pending_recv = pending_recv;
    }
}

#[cfg(test)]
fn offer_rollback_selection_for_test() -> OfferScopeSelection {
    OfferScopeSelection {
        scope_id: crate::global::const_dsl::ScopeId::none(),
        frontier_parallel_root: None,
        offer_lane: 0,
        offer_lane_idx: 0,
        at_route_offer_entry: false,
    }
}

#[cfg(test)]
fn offer_rollback_facts_for_test(selection: OfferScopeSelection) -> OfferFrontierFacts {
    OfferFrontierFacts {
        selection,
        scope_id: selection.scope_id,
        offer_lane_idx: selection.offer_lane_idx as usize,
        suppress_scope_frame_hint: false,
        is_route_controller: false,
        is_dynamic_route_scope: false,
        ingress_mode: OfferIngressMode::ProbeSelectedBinding,
    }
}
