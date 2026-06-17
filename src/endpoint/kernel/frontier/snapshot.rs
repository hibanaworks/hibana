use super::{FrontierCandidate, FrontierKind, FrontierScratchView, OfferEntryEvidence, ScopeId};
pub(crate) fn frontier_snapshot_from_scratch(
    scratch: &mut FrontierScratchView,
    current_scope: ScopeId,
    current_entry_idx: usize,
    current_parallel_root: ScopeId,
    current_frontier: FrontierKind,
) -> FrontierSnapshot {
    let candidate_capacity = scratch.candidates_mut().len();
    /* SAFETY: `scratch` is the endpoint frontier scratch borrow for the active
    operation. Its candidate slice remains live for the returned snapshot, and
    the snapshot initializes at most `candidate_capacity` cells. */
    unsafe {
        FrontierSnapshot::from_parts(
            current_scope,
            current_entry_idx,
            current_parallel_root,
            current_frontier,
            scratch.candidates_mut().as_mut_ptr(),
            candidate_capacity,
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FrontierSnapshot {
    pub(crate) current_scope: ScopeId,
    pub(crate) current_entry_idx: usize,
    pub(crate) current_parallel_root: ScopeId,
    pub(crate) current_frontier: FrontierKind,
    candidates: *mut FrontierCandidate,
    candidate_capacity: usize,
    pub(crate) candidate_len: usize,
}

impl FrontierSnapshot {
    #[inline]
    pub(crate) unsafe fn from_parts(
        current_scope: ScopeId,
        current_entry_idx: usize,
        current_parallel_root: ScopeId,
        current_frontier: FrontierKind,
        candidates: *mut FrontierCandidate,
        candidate_capacity: usize,
    ) -> Self {
        let mut idx = 0usize;
        while idx < candidate_capacity {
            /* SAFETY: `idx < candidate_capacity` bounds the scratch candidate
            buffer owned by this snapshot; every slot is reset before
            `candidate_len` exposes any candidate. */
            unsafe {
                candidates.add(idx).write(FrontierCandidate::EMPTY);
            }
            idx += 1;
        }
        Self {
            current_scope,
            current_entry_idx,
            current_parallel_root,
            current_frontier,
            candidates,
            candidate_capacity,
            candidate_len: 0,
        }
    }

    #[inline]
    pub(crate) fn push_candidate(&mut self, candidate: FrontierCandidate) -> bool {
        if self.candidate_len >= self.candidate_capacity {
            return false;
        }
        /* SAFETY: `candidate_len < candidate_capacity` bounds the next
        snapshot candidate slot; the length is increased only after the write. */
        unsafe {
            self.candidates.add(self.candidate_len).write(candidate);
        }
        self.candidate_len += 1;
        true
    }

    #[inline]
    fn candidate_at(self, idx: usize) -> FrontierCandidate {
        if idx >= self.candidate_len {
            crate::invariant();
        }
        /* SAFETY: `idx < candidate_len` bounds the initialized prefix of the
        snapshot's scratch candidate buffer; this copies one candidate without
        mutating the scratch slice. */
        unsafe { *self.candidates.add(idx) }
    }

    #[inline]
    pub(crate) fn matches_parallel_root(self, candidate: FrontierCandidate) -> bool {
        self.current_parallel_root.is_none()
            || candidate.parallel_root == self.current_parallel_root
    }

    pub(crate) fn select_yield_candidate(
        self,
        visited: FrontierVisitSet,
    ) -> Option<FrontierCandidate> {
        let mut idx = 0usize;
        while idx < self.candidate_len {
            let candidate = self.candidate_at(idx);
            if (candidate.scope_id != self.current_scope
                || candidate.entry_idx as usize != self.current_entry_idx)
                && self.matches_parallel_root(candidate)
                && candidate.frontier == self.current_frontier
                && candidate.ready()
                && candidate.has_evidence()
                && !visited.contains(candidate.scope_id)
            {
                return Some(candidate);
            }
            idx += 1;
        }
        idx = 0;
        while idx < self.candidate_len {
            let candidate = self.candidate_at(idx);
            if (candidate.scope_id != self.current_scope
                || candidate.entry_idx as usize != self.current_entry_idx)
                && self.matches_parallel_root(candidate)
                && candidate.ready()
                && candidate.has_evidence()
                && !visited.contains(candidate.scope_id)
            {
                return Some(candidate);
            }
            idx += 1;
        }
        None
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FrontierVisitSet {
    slots: *mut ScopeId,
    capacity: usize,
    pub(crate) len: usize,
}

impl FrontierVisitSet {
    #[inline]
    pub(crate) unsafe fn from_parts(slots: *mut ScopeId, capacity: usize) -> Self {
        let mut idx = 0usize;
        while idx < capacity {
            /* SAFETY: `idx < capacity` bounds the scratch visited-scope
            buffer. All cells are reset before `len` exposes the initialized
            prefix. */
            unsafe {
                slots.add(idx).write(ScopeId::none());
            }
            idx += 1;
        }
        Self {
            slots,
            capacity,
            len: 0,
        }
    }

    #[inline]
    pub(crate) fn contains(self, scope: ScopeId) -> bool {
        let mut idx = 0usize;
        while idx < self.len {
            if
            /* SAFETY: `idx < len` bounds the initialized prefix of the
            visited-scope scratch buffer; this shared read copies one scope id. */
            unsafe { *self.slots.add(idx) } == scope {
                return true;
            }
            idx += 1;
        }
        false
    }

    #[inline]
    pub(crate) fn record(&mut self, scope: ScopeId) {
        if scope.is_none() || self.contains(scope) || self.len >= self.capacity {
            return;
        }
        /* SAFETY: `len < capacity` bounds the next visited-scope slot; the
        initialized prefix grows only after this write. */
        unsafe {
            self.slots.add(self.len).write(scope);
        }
        self.len += 1;
    }
}

#[inline]
pub(crate) fn frontier_visit_set_from_scratch(
    scratch: &mut FrontierScratchView,
) -> FrontierVisitSet {
    let capacity = scratch.visited_scopes_mut().len();
    /* SAFETY: `scratch` is the endpoint frontier scratch borrow for the active
    operation. The visited-scope slice remains live for the returned set and is
    reset by `from_parts`. */
    unsafe { FrontierVisitSet::from_parts(scratch.visited_scopes_mut().as_mut_ptr(), capacity) }
}

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
        if evidence.has_ack() {
            bits |= 1 << 0;
        }
        if evidence.has_ready_arm() {
            bits |= 1 << 1;
        }
        if evidence.ingress_ready() {
            bits |= 1 << 2;
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
pub(crate) const fn align_up(value: usize, align: usize) -> usize {
    let mask = match align.checked_sub(1) {
        Some(mask) => mask,
        None => crate::invariant(),
    };
    (value + mask) & !mask
}

#[inline(always)]
pub(crate) const fn max_usize(lhs: usize, rhs: usize) -> usize {
    if lhs > rhs { lhs } else { rhs }
}
