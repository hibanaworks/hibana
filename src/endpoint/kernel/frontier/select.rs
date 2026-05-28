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
    binding_ready: bool,
) -> bool {
    has_ready_arm_evidence || ack_is_progress || binding_ready
}

#[inline]
pub(crate) fn offer_entry_observed_state(
    _scope_id: ScopeId,
    summary: OfferEntryStaticSummary,
    has_ready_arm_evidence: bool,
    ack_is_progress: bool,
    binding_ready: bool,
) -> OfferEntryObservedState {
    let has_progress_evidence =
        candidate_has_progress_evidence(has_ready_arm_evidence, ack_is_progress, binding_ready);
    let ready =
        has_ready_arm_evidence || ack_is_progress || binding_ready || summary.static_ready();
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
    if binding_ready {
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

#[cfg(test)]
#[inline]
pub(crate) fn record_offer_entry_reentry_candidate(
    current_scope: ScopeId,
    current_entry_idx: usize,
    current_parallel_root: ScopeId,
    candidate: FrontierCandidate,
    ready_entry_idx: &mut Option<usize>,
    any_entry_idx: &mut Option<usize>,
) {
    if (candidate.scope_id == current_scope && candidate.entry_idx as usize == current_entry_idx)
        || (!current_parallel_root.is_none() && candidate.parallel_root != current_parallel_root)
    {
        return;
    }
    if any_entry_idx.is_none() {
        *any_entry_idx = Some(candidate.entry_idx as usize);
    }
    if candidate.ready() && ready_entry_idx.is_none() {
        *ready_entry_idx = Some(candidate.entry_idx as usize);
    }
}
