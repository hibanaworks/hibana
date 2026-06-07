use super::{
    FrontierCandidate, FrontierKind, ObservedEntrySet, OfferEntryObservedState,
    OfferEntryStaticSummary, ScopeId, TryFrom,
};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OfferSelectPriority {
    CurrentOfferEntry,
    DynamicControllerUnique,
    ControllerUnique,
    CandidateUnique,
}

#[inline]
pub(crate) fn candidate_has_progress_evidence(
    has_ready_arm_evidence: bool,
    ack_is_progress: bool,
    ingress_ready: bool,
) -> bool {
    has_ready_arm_evidence || ack_is_progress || ingress_ready
}

#[inline]
pub(crate) fn offer_entry_observed_state(
    _scope_id: ScopeId,
    summary: OfferEntryStaticSummary,
    has_ready_arm_evidence: bool,
    ack_is_progress: bool,
    ingress_ready: bool,
) -> OfferEntryObservedState {
    let has_progress_evidence =
        candidate_has_progress_evidence(has_ready_arm_evidence, ack_is_progress, ingress_ready);
    let ready =
        has_ready_arm_evidence || ack_is_progress || ingress_ready || summary.static_ready();
    let mut flags = 0u8;
    if summary.is_controller() {
        flags |= OfferEntryObservedState::FLAG_CONTROLLER;
    }
    if summary.is_dynamic() {
        flags |= OfferEntryObservedState::FLAG_DYNAMIC;
    }
    if has_progress_evidence {
        flags |= OfferEntryObservedState::FLAG_PROGRESS;
    }
    if has_ready_arm_evidence {
        flags |= OfferEntryObservedState::FLAG_READY_ARM;
    }
    if ingress_ready {
        flags |= OfferEntryObservedState::FLAG_BINDING_READY;
    }
    if ready {
        flags |= OfferEntryObservedState::FLAG_READY;
    }
    OfferEntryObservedState {
        scope_id: _scope_id,
        frontier_mask: summary.frontier_mask,
        flags,
    }
}

#[inline]
pub(crate) fn offer_entry_frontier_candidate(
    scope_id: ScopeId,
    entry_idx: usize,
    parallel_root: ScopeId,
    frontier: FrontierKind,
    observed: OfferEntryObservedState,
) -> FrontierCandidate {
    debug_assert!(
        u16::try_from(entry_idx).is_ok(),
        "offer entry index must fit u16"
    );
    FrontierCandidate {
        scope_id,
        entry_idx: entry_idx as u16,
        parallel_root,
        frontier,
        flags: FrontierCandidate::pack_flags(
            observed.is_controller(),
            observed.is_dynamic(),
            observed.has_progress_evidence(),
            observed.ready(),
        ),
    }
}

#[inline]
pub(crate) fn cached_offer_entry_observed_state(
    _scope_id: ScopeId,
    summary: OfferEntryStaticSummary,
    observed_entries: ObservedEntrySet,
    observed_bit: u8,
) -> OfferEntryObservedState {
    let mut flags = 0u8;
    if summary.is_controller() {
        flags |= OfferEntryObservedState::FLAG_CONTROLLER;
    }
    if summary.is_dynamic() {
        flags |= OfferEntryObservedState::FLAG_DYNAMIC;
    }
    if (observed_entries.progress_mask & observed_bit) != 0 {
        flags |= OfferEntryObservedState::FLAG_PROGRESS;
    }
    if (observed_entries.ready_arm_mask & observed_bit) != 0 {
        flags |= OfferEntryObservedState::FLAG_READY_ARM;
    }
    if (observed_entries.ready_mask & observed_bit) != 0 {
        flags |= OfferEntryObservedState::FLAG_READY;
    }
    OfferEntryObservedState {
        scope_id: _scope_id,
        frontier_mask: summary.frontier_mask,
        flags,
    }
}
