use super::{
    FrontierKind, FrontierProgressCandidate, FrontierVisitSet, ScopeId, StateIndex,
    checked_state_index,
};

#[derive(Clone, Copy, Debug)]
pub(crate) struct FrontierProgressSelection {
    current_scope: ScopeId,
    current_entry: StateIndex,
    current_parallel_root: ScopeId,
    current_frontier: FrontierKind,
    preferred: Option<FrontierProgressCandidate>,
    first_admissible: Option<FrontierProgressCandidate>,
}

impl FrontierProgressSelection {
    #[inline]
    pub(crate) fn new(
        current_scope: ScopeId,
        current_entry_idx: usize,
        current_parallel_root: ScopeId,
        current_frontier: FrontierKind,
    ) -> Self {
        Self {
            current_scope,
            current_entry: crate::invariant_some(checked_state_index(current_entry_idx)),
            current_parallel_root,
            current_frontier,
            preferred: None,
            first_admissible: None,
        }
    }

    #[inline]
    fn matches_parallel_root(&self, candidate: FrontierProgressCandidate) -> bool {
        self.current_parallel_root.is_none()
            || candidate.parallel_root == self.current_parallel_root
    }

    #[inline]
    fn candidate_is_admissible(
        &self,
        candidate: FrontierProgressCandidate,
        visited: &FrontierVisitSet,
    ) -> bool {
        (candidate.scope_id != self.current_scope || candidate.entry != self.current_entry)
            && self.matches_parallel_root(candidate)
            && !visited.contains(candidate.entry.as_usize())
    }

    #[inline]
    pub(crate) fn consider(
        &mut self,
        candidate: FrontierProgressCandidate,
        visited: &FrontierVisitSet,
    ) {
        if !self.candidate_is_admissible(candidate, visited) {
            return;
        }
        if self.first_admissible.is_none() {
            self.first_admissible = Some(candidate);
        }
        if candidate.frontier == self.current_frontier && self.preferred.is_none() {
            self.preferred = Some(candidate);
        }
    }

    #[inline]
    pub(crate) const fn selected(self) -> Option<FrontierProgressCandidate> {
        match self.preferred {
            Some(preferred) => Some(preferred),
            None => self.first_admissible,
        }
    }
}

#[cfg(all(test, hibana_repo_tests))]
mod tests;

#[cfg(kani)]
mod kani;
