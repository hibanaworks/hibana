use super::{
    FrontierCandidate, FrontierKind, ObservedEntrySet, OfferEntryObservedState, OfferEntrySummary,
    ScopeId, TryFrom,
};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OfferSelectPriority {
    CurrentOfferEntry,
    DynamicControllerUnique,
    ControllerUnique,
    CandidateUnique,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct OfferEntryEvidence {
    bits: u8,
}

impl OfferEntryEvidence {
    pub(crate) const FLAG_ACK: u8 = 1;
    pub(crate) const FLAG_READY_ARM: u8 = 1 << 1;
    pub(crate) const FLAG_INGRESS_READY: u8 = 1 << 2;

    #[inline]
    pub(crate) const fn empty() -> Self {
        Self { bits: 0 }
    }

    #[inline]
    pub(crate) const fn with_ack(self) -> Self {
        Self {
            bits: self.bits | Self::FLAG_ACK,
        }
    }

    #[inline]
    pub(crate) const fn with_ready_arm(self) -> Self {
        Self {
            bits: self.bits | Self::FLAG_READY_ARM,
        }
    }

    #[inline]
    pub(crate) const fn with_ingress_ready(self) -> Self {
        Self {
            bits: self.bits | Self::FLAG_INGRESS_READY,
        }
    }

    #[inline]
    pub(crate) const fn has_ack(self) -> bool {
        (self.bits & Self::FLAG_ACK) != 0
    }

    #[inline]
    pub(crate) const fn has_ready_arm(self) -> bool {
        (self.bits & Self::FLAG_READY_ARM) != 0
    }

    #[inline]
    pub(crate) const fn ingress_ready(self) -> bool {
        (self.bits & Self::FLAG_INGRESS_READY) != 0
    }
}

#[inline]
pub(crate) fn candidate_has_progress_evidence(evidence: OfferEntryEvidence) -> bool {
    evidence.has_ready_arm() || evidence.has_ack() || evidence.ingress_ready()
}

#[inline]
pub(crate) fn offer_entry_observed_state(
    scope_id: ScopeId,
    summary: OfferEntrySummary,
    evidence: OfferEntryEvidence,
) -> OfferEntryObservedState {
    let has_progress_evidence = candidate_has_progress_evidence(evidence);
    let ready = has_progress_evidence || summary.intrinsic_ready();
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
    if evidence.has_ready_arm() {
        flags |= OfferEntryObservedState::FLAG_READY_ARM;
    }
    if evidence.ingress_ready() {
        flags |= OfferEntryObservedState::FLAG_BINDING_READY;
    }
    if ready {
        flags |= OfferEntryObservedState::FLAG_READY;
    }
    OfferEntryObservedState {
        scope_id,
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
    if u16::try_from(entry_idx).is_err() {
        crate::invariant();
    }
    FrontierCandidate {
        scope_id,
        entry_idx: entry_idx as u16,
        parallel_root,
        frontier,
        flags: FrontierCandidate::flags_from_observed(observed),
    }
}

#[inline]
pub(crate) fn cached_offer_entry_observed_state(
    scope_id: ScopeId,
    summary: OfferEntrySummary,
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
        scope_id,
        frontier_mask: summary.frontier_mask,
        flags,
    }
}
