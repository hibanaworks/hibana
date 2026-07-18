use super::super::{FrontierKind, LaneOfferState, OfferEntryObservedState, ScopeId, StateIndex};
use super::{
    OfferEntryEvidence, offer_entry_frontier_progress_candidate, offer_entry_observed_state,
};

fn symbolic_offer_state() -> LaneOfferState {
    let mut flags = 0;
    if kani::any::<bool>() {
        flags |= LaneOfferState::FLAG_CONTROLLER;
    }
    if kani::any::<bool>() {
        flags |= LaneOfferState::FLAG_DYNAMIC;
    }
    if kani::any::<bool>() {
        flags |= LaneOfferState::FLAG_INTRINSIC_READY;
    }
    LaneOfferState {
        scope: ScopeId::route(1),
        entry: StateIndex::new(kani::any::<u8>() as u16),
        parallel_root: ScopeId::none(),
        frontier: kani::any::<FrontierKind>(),
        flags,
    }
}

#[kani::proof]
fn progress_candidate_exists_exactly_for_production_progress_evidence() {
    let info = symbolic_offer_state();
    let has_ready_arm = kani::any::<bool>();
    let has_ingress = kani::any::<bool>();
    let mut evidence = OfferEntryEvidence::empty();
    if has_ready_arm {
        evidence = evidence.with_ready_arm();
    }
    if has_ingress {
        evidence = evidence.with_ingress_ready();
    }
    let observed = offer_entry_observed_state(info, evidence);
    let candidate = offer_entry_frontier_progress_candidate(info, observed);

    assert_eq!(candidate.is_some(), has_ready_arm || has_ingress);
    if let Some(candidate) = candidate {
        assert_eq!(candidate.scope_id, info.scope);
        assert_eq!(candidate.entry, info.entry);
        assert_eq!(candidate.parallel_root, info.parallel_root);
        assert_eq!(candidate.frontier, info.frontier);
    }
}

#[kani::proof]
#[kani::should_panic]
fn progress_candidate_rejects_evidence_without_readiness() {
    let info = symbolic_offer_state();
    let observed = OfferEntryObservedState {
        key: crate::invariant_some(info.key()),
        frontier_mask: info.frontier.bit(),
        flags: OfferEntryObservedState::FLAG_PROGRESS,
    };

    let _ = offer_entry_frontier_progress_candidate(info, observed);
}
