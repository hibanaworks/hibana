use super::{FrontierCandidate, LaneOfferState, OfferEntryObservedState};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct OfferEntryEvidence {
    bits: u8,
}

impl OfferEntryEvidence {
    pub(crate) const FLAG_READY_ARM: u8 = 1;
    pub(crate) const FLAG_INGRESS_READY: u8 = 1 << 1;

    #[inline]
    pub(crate) const fn empty() -> Self {
        Self { bits: 0 }
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
    evidence.has_ready_arm() || evidence.ingress_ready()
}

#[inline]
pub(crate) fn offer_entry_observed_state(
    info: LaneOfferState,
    evidence: OfferEntryEvidence,
) -> OfferEntryObservedState {
    let has_progress_evidence = candidate_has_progress_evidence(evidence);
    let ready = has_progress_evidence || info.intrinsic_ready();
    let mut flags = 0u8;
    if info.is_controller() {
        flags |= OfferEntryObservedState::FLAG_CONTROLLER;
    }
    if info.is_dynamic() {
        flags |= OfferEntryObservedState::FLAG_DYNAMIC;
    }
    if has_progress_evidence {
        flags |= OfferEntryObservedState::FLAG_PROGRESS;
    }
    if evidence.has_ready_arm() {
        flags |= OfferEntryObservedState::FLAG_READY_ARM;
    }
    if ready {
        flags |= OfferEntryObservedState::FLAG_READY;
    }
    OfferEntryObservedState {
        key: crate::invariant_some(info.key()),
        frontier_mask: info.frontier.bit(),
        flags,
    }
}

#[inline]
pub(crate) fn offer_entry_frontier_candidate(
    info: LaneOfferState,
    observed: OfferEntryObservedState,
) -> FrontierCandidate {
    if Some(observed.key) != info.key() {
        crate::invariant();
    }
    FrontierCandidate {
        scope_id: info.scope,
        entry: observed.key.entry(),
        parallel_root: info.parallel_root,
        frontier: info.frontier,
        flags: FrontierCandidate::flags_from_observed(observed),
    }
}
