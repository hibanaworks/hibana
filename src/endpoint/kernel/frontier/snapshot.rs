#[cfg(all(test, hibana_repo_tests))]
use super::MAX_STATES;
use super::{
    FrontierCandidate, FrontierKind, FrontierScratchSectionLease, OfferEntryEvidence, ScopeId,
    StateIndex, checked_state_index, frontier_candidates_mut,
};
pub(crate) fn frontier_snapshot_from_scratch<'a>(
    candidates: &'a mut FrontierScratchSectionLease<'_, FrontierCandidate>,
    current_scope: ScopeId,
    current_entry_idx: usize,
    current_parallel_root: ScopeId,
    current_frontier: FrontierKind,
) -> FrontierSnapshot<'a> {
    let current_entry = crate::invariant_some(checked_state_index(current_entry_idx));
    FrontierSnapshot::new(
        current_scope,
        current_entry,
        current_parallel_root,
        current_frontier,
        frontier_candidates_mut(candidates),
    )
}

#[derive(Debug)]
pub(crate) struct FrontierSnapshot<'a> {
    pub(crate) current_scope: ScopeId,
    pub(crate) current_entry: StateIndex,
    pub(crate) current_parallel_root: ScopeId,
    pub(crate) current_frontier: FrontierKind,
    candidates: &'a mut [FrontierCandidate],
    pub(crate) candidate_len: usize,
}

impl<'a> FrontierSnapshot<'a> {
    #[inline]
    pub(crate) fn new(
        current_scope: ScopeId,
        current_entry: StateIndex,
        current_parallel_root: ScopeId,
        current_frontier: FrontierKind,
        candidates: &'a mut [FrontierCandidate],
    ) -> Self {
        if candidates.len() > u16::MAX as usize {
            crate::invariant();
        }
        candidates.fill(FrontierCandidate::EMPTY);
        Self {
            current_scope,
            current_entry,
            current_parallel_root,
            current_frontier,
            candidates,
            candidate_len: 0,
        }
    }

    #[inline]
    pub(crate) fn push_candidate(&mut self, candidate: FrontierCandidate) -> bool {
        let Some(slot) = self.candidates.get_mut(self.candidate_len) else {
            return false;
        };
        *slot = candidate;
        self.candidate_len += 1;
        true
    }

    #[inline]
    fn candidate_at(&self, idx: usize) -> FrontierCandidate {
        if idx >= self.candidate_len {
            crate::invariant();
        }
        *crate::invariant_some(self.candidates.get(idx))
    }

    #[inline]
    pub(crate) fn matches_parallel_root(&self, candidate: FrontierCandidate) -> bool {
        self.current_parallel_root.is_none()
            || candidate.parallel_root == self.current_parallel_root
    }

    pub(crate) fn select_yield_candidate(
        &self,
        visited: &FrontierVisitSet,
    ) -> Option<FrontierCandidate> {
        let mut idx = 0usize;
        while idx < self.candidate_len {
            let candidate = self.candidate_at(idx);
            if (candidate.scope_id != self.current_scope || candidate.entry != self.current_entry)
                && self.matches_parallel_root(candidate)
                && candidate.frontier == self.current_frontier
                && candidate.ready()
                && candidate.has_evidence()
                && !visited.contains(candidate.entry.as_usize())
            {
                return Some(candidate);
            }
            idx += 1;
        }
        idx = 0;
        while idx < self.candidate_len {
            let candidate = self.candidate_at(idx);
            if (candidate.scope_id != self.current_scope || candidate.entry != self.current_entry)
                && self.matches_parallel_root(candidate)
                && candidate.ready()
                && candidate.has_evidence()
                && !visited.contains(candidate.entry.as_usize())
            {
                return Some(candidate);
            }
            idx += 1;
        }
        None
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct FrontierVisitSet {
    slots: *mut StateIndex,
    capacity: u16,
    len: u16,
}

impl FrontierVisitSet {
    const EMPTY: Self = Self {
        slots: core::ptr::null_mut(),
        capacity: 0,
        len: 0,
    };

    #[inline]
    pub(crate) unsafe fn from_parts(slots: *mut StateIndex, capacity: usize) -> Self {
        if capacity > u16::MAX as usize || (capacity != 0 && slots.is_null()) {
            crate::invariant();
        }
        let mut idx = 0usize;
        while idx < capacity {
            /* SAFETY: `idx < capacity` bounds the resident visited-entry
            buffer. All cells are reset before `len` exposes the initialized
            prefix. */
            unsafe {
                slots.add(idx).write(StateIndex::ABSENT);
            }
            idx += 1;
        }
        Self {
            slots,
            capacity: capacity as u16,
            len: 0,
        }
    }

    #[inline]
    pub(crate) fn contains(&self, entry_idx: usize) -> bool {
        let Some(entry) = checked_state_index(entry_idx) else {
            crate::invariant();
        };
        let mut idx = 0usize;
        while idx < self.len as usize {
            if
            /* SAFETY: `idx < len` bounds the initialized prefix of the
            visited-entry buffer; this shared read copies one state identity. */
            unsafe { *self.slots.add(idx) } == entry {
                return true;
            }
            idx += 1;
        }
        false
    }

    #[inline]
    pub(crate) fn record(&mut self, entry_idx: usize) {
        if self.contains(entry_idx) {
            return;
        }
        if self.len >= self.capacity {
            crate::invariant();
        }
        let entry = crate::invariant_some(checked_state_index(entry_idx));
        /* SAFETY: `len < capacity` bounds the next visited-entry slot; the
        initialized prefix grows only after this write. */
        unsafe {
            self.slots.add(self.len as usize).write(entry);
        }
        self.len += 1;
    }

    #[inline]
    pub(crate) fn take(&mut self) -> Self {
        core::mem::replace(self, Self::EMPTY)
    }

    #[cfg(kani)]
    #[inline]
    pub(crate) const fn len(&self) -> usize {
        self.len as usize
    }
}

#[cfg(all(test, hibana_repo_tests))]
mod tests {
    use super::*;

    #[test]
    fn visit_set_distinguishes_entries_that_share_a_scope() {
        let scope = ScopeId::route(1);
        let mut candidates = [FrontierCandidate::EMPTY; 2];
        let mut snapshot = FrontierSnapshot::new(
            ScopeId::none(),
            StateIndex::new(0),
            ScopeId::none(),
            FrontierKind::Route,
            &mut candidates,
        );
        for entry_idx in [1u16, 2u16] {
            assert!(snapshot.push_candidate(FrontierCandidate {
                scope_id: scope,
                entry: StateIndex::new(entry_idx),
                parallel_root: ScopeId::none(),
                frontier: FrontierKind::Route,
                flags: FrontierCandidate::FLAG_HAS_EVIDENCE | FrontierCandidate::FLAG_READY,
            }));
        }
        let mut visited_storage = [StateIndex::ABSENT; 2];
        /* SAFETY: `visited_storage` is initialized, live, and exclusively
        borrowed for the complete visit-set use. */
        let mut visited = unsafe {
            FrontierVisitSet::from_parts(visited_storage.as_mut_ptr(), visited_storage.len())
        };
        visited.record(1);

        assert_eq!(
            snapshot
                .select_yield_candidate(&visited)
                .map(|candidate| candidate.entry.raw()),
            Some(2)
        );
    }

    #[test]
    #[should_panic]
    fn visit_set_fails_closed_instead_of_truncating() {
        let mut storage = [StateIndex::ABSENT; 1];
        /* SAFETY: `storage` is one initialized, live, exclusively borrowed
        visit cell whose exact length is passed to the view. */
        let mut visited =
            unsafe { FrontierVisitSet::from_parts(storage.as_mut_ptr(), storage.len()) };
        visited.record(1);
        visited.record(2);
    }

    #[test]
    fn visit_set_holds_the_current_entry_and_the_full_active_frontier() {
        const ACTIVE_LANE_COUNT: usize = u8::MAX as usize + 1;
        let mut storage = [StateIndex::ABSENT; ACTIVE_LANE_COUNT + 1];
        /* SAFETY: `storage` is initialized, live, and exclusively borrowed for
        the complete visit-set use. Its extra cell is the current cursor entry. */
        let mut visited =
            unsafe { FrontierVisitSet::from_parts(storage.as_mut_ptr(), storage.len()) };
        let current = MAX_STATES - 1;
        visited.record(current);
        let mut entry = 0usize;
        while entry < ACTIVE_LANE_COUNT {
            visited.record(entry);
            entry += 1;
        }

        assert_eq!(visited.len as usize, ACTIVE_LANE_COUNT + 1);
        assert!(visited.contains(current));
        assert!(visited.contains(ACTIVE_LANE_COUNT - 1));
    }

    #[test]
    fn absent_state_identity_is_not_admissible() {
        assert!(checked_state_index(MAX_STATES - 1).is_some());
        assert!(checked_state_index(MAX_STATES).is_none());
        assert!(checked_state_index(usize::MAX).is_none());
    }
}

#[cfg(kani)]
mod kani;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FrontierDeferOutcome {
    Continue,
    Yielded,
    Pending,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct EvidenceFingerprint(u8);

impl EvidenceFingerprint {
    #[inline]
    pub(crate) const fn from_offer_entry_evidence(evidence: OfferEntryEvidence) -> Self {
        let mut bits = 0u8;
        if evidence.has_ready_arm() {
            bits |= 1 << 0;
        }
        if evidence.ingress_ready() {
            bits |= 1 << 1;
        }
        Self(bits)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct OfferProgressState {
    pub(crate) last_fingerprint: Option<EvidenceFingerprint>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OfferEvidenceOutcome {
    NewEvidence,
    Pending,
}

impl OfferProgressState {
    #[inline]
    pub(crate) const fn new() -> Self {
        Self {
            last_fingerprint: None,
        }
    }

    #[inline]
    pub(crate) fn on_defer(&mut self, fingerprint: EvidenceFingerprint) -> OfferEvidenceOutcome {
        let has_new_evidence = self.last_fingerprint != Some(fingerprint);
        self.last_fingerprint = Some(fingerprint);
        if has_new_evidence {
            OfferEvidenceOutcome::NewEvidence
        } else {
            OfferEvidenceOutcome::Pending
        }
    }
}

#[inline(always)]
pub(crate) const fn max_usize(lhs: usize, rhs: usize) -> usize {
    if lhs > rhs { lhs } else { rhs }
}
