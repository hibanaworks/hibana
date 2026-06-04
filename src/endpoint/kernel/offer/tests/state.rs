use super::super::{OfferScopeProfile, ingress::OfferIngressMode};
use super::*;

impl<'a> OfferStagedIngress<'a> {
    #[inline]
    pub(super) const fn new(
        binding_evidence: Option<LaneIngressEvidence>,
        transport_payload: Option<lane_port::PreambleFrame<'a>>,
    ) -> Self {
        Self {
            binding_evidence,
            transport_payload,
        }
    }
}

impl<'r> OfferState<'r> {
    pub(in crate::endpoint::kernel) fn install_carried_binding_evidence(
        &mut self,
        evidence: LaneIngressEvidence,
    ) {
        self.carried_ingress.stage_binding(evidence);
    }

    pub(in crate::endpoint::kernel) fn install_carried_transport_frame(
        &mut self,
        frame: lane_port::PreambleFrame<'r>,
    ) {
        self.carried_ingress.stage_transport(frame);
    }

    pub(in crate::endpoint::kernel) fn install_collecting_rollback_items(
        &mut self,
        binding_evidence: Option<LaneIngressEvidence>,
        transport_payload: Option<lane_port::PreambleFrame<'r>>,
    ) {
        let selection = offer_rollback_selection();
        self.execution = OfferExecution::Collecting {
            frontier_visited: super::super::FrontierVisitSet::empty(),
            stage: OfferCollectState {
                facts: offer_rollback_facts(selection),
                ingress: OfferStagedIngress::new(binding_evidence, transport_payload),
            },
        };
    }

    pub(in crate::endpoint::kernel) fn install_resolving_rollback_items(
        &mut self,
        binding_evidence: Option<LaneIngressEvidence>,
        transport_payload: Option<lane_port::PreambleFrame<'r>>,
    ) {
        let selection = offer_rollback_selection();
        self.execution = OfferExecution::Resolving {
            frontier_visited: super::super::FrontierVisitSet::empty(),
            stage: OfferResolveState {
                facts: offer_rollback_facts(selection),
                ingress: OfferStagedIngress::new(binding_evidence, transport_payload),
                progress: OfferProgressState::new(crate::runtime::config::OfferProgressPolicy),
                pending: ResolvePendingState::ready(),
            },
        };
    }
}

fn offer_rollback_selection() -> OfferScopeSelection {
    OfferScopeSelection {
        scope_id: crate::global::const_dsl::ScopeId::none(),
        frontier_parallel_root: None,
        offer_lane: 0,
        at_route_offer_entry: false,
    }
}

fn offer_rollback_facts(selection: OfferScopeSelection) -> OfferFrontierFacts {
    OfferFrontierFacts {
        selection,
        profile: OfferScopeProfile::PassiveStatic,
        ingress_mode: OfferIngressMode::ProbeSelectedBinding,
    }
}
