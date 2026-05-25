use super::*;

pub(crate) fn frontier_snapshot_from_scratch(
    scratch: &mut FrontierScratchView,
    current_scope: ScopeId,
    current_entry_idx: usize,
    current_parallel_root: ScopeId,
    current_frontier: FrontierKind,
) -> FrontierSnapshot {
    let candidate_capacity = scratch.candidates_mut().len();
    /* SAFETY: endpoint kernel owns the resident endpoint storage and holds the affine operation borrow for this raw access. */
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
            /* SAFETY: initialization owns exclusive writable storage for this field and writes it exactly once before exposure. */
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
        /* SAFETY: initialization owns exclusive writable storage for this field and writes it exactly once before exposure. */
        unsafe {
            self.candidates.add(self.candidate_len).write(candidate);
        }
        self.candidate_len += 1;
        true
    }

    #[inline]
    fn candidate_at(self, idx: usize) -> FrontierCandidate {
        debug_assert!(idx < self.candidate_len);
        /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */
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
            /* SAFETY: initialization owns exclusive writable storage for this field and writes it exactly once before exposure. */
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
            /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */
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
        /* SAFETY: initialization owns exclusive writable storage for this field and writes it exactly once before exposure. */
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
    /* SAFETY: endpoint kernel owns the resident endpoint storage and holds the affine operation borrow for this raw access. */
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
    pub(crate) const fn new(
        has_ack: bool,
        has_ready_arm_evidence: bool,
        binding_ready: bool,
    ) -> Self {
        let mut bits = 0u8;
        if has_ack {
            bits |= 1 << 0;
        }
        if has_ready_arm_evidence {
            bits |= 1 << 1;
        }
        if binding_ready {
            bits |= 1 << 2;
        }
        Self(bits)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct OfferProgressState {
    pub(crate) policy: crate::runtime::config::OfferProgressPolicy,
    pub(crate) last_fingerprint: Option<EvidenceFingerprint>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OfferEvidenceOutcome {
    NewEvidence,
    Pending,
}

impl OfferProgressState {
    #[inline]
    pub(crate) fn new(policy: crate::runtime::config::OfferProgressPolicy) -> Self {
        Self {
            policy,
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
    let mask = align.saturating_sub(1);
    (value + mask) & !mask
}

#[inline(always)]
pub(crate) const fn max_usize(lhs: usize, rhs: usize) -> usize {
    if lhs > rhs { lhs } else { rhs }
}
