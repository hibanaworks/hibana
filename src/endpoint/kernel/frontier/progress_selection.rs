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
    pub(crate) fn consider(
        &mut self,
        candidate: FrontierProgressCandidate,
        visited: &FrontierVisitSet,
    ) {
        if (candidate.scope_id == self.current_scope && candidate.entry == self.current_entry)
            || !self.matches_parallel_root(candidate)
            || visited.contains(candidate.entry.as_usize())
        {
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
mod tests {
    use super::*;

    fn candidate(entry: u16, frontier: FrontierKind) -> FrontierProgressCandidate {
        FrontierProgressCandidate {
            scope_id: ScopeId::route(1),
            entry: StateIndex::new(entry),
            parallel_root: ScopeId::none(),
            frontier,
        }
    }

    #[test]
    fn progress_selection_distinguishes_entries_that_share_a_scope() {
        let mut selection = FrontierProgressSelection::new(
            ScopeId::none(),
            0,
            ScopeId::none(),
            FrontierKind::Route,
        );
        let mut visited_storage = [StateIndex::ABSENT; 2];
        /* SAFETY: `visited_storage` is initialized, live, and exclusively
        borrowed for the complete visit-set use. */
        let mut visited = unsafe {
            FrontierVisitSet::from_parts(visited_storage.as_mut_ptr(), visited_storage.len())
        };
        visited.record(1);
        selection.consider(candidate(1, FrontierKind::Route), &visited);
        selection.consider(candidate(2, FrontierKind::Route), &visited);

        assert_eq!(
            selection.selected().map(|candidate| candidate.entry.raw()),
            Some(2)
        );
    }

    #[test]
    fn progress_selection_prefers_matching_frontier_without_candidate_storage() {
        let mut selection = FrontierProgressSelection::new(
            ScopeId::none(),
            0,
            ScopeId::none(),
            FrontierKind::Route,
        );
        let visited = FrontierVisitSet::EMPTY;
        selection.consider(candidate(1, FrontierKind::Reentry), &visited);
        selection.consider(candidate(2, FrontierKind::Route), &visited);
        selection.consider(candidate(3, FrontierKind::Route), &visited);

        assert_eq!(
            selection.selected().map(|candidate| candidate.entry.raw()),
            Some(2)
        );
    }

    #[test]
    fn progress_selection_retains_first_admissible_without_matching_frontier() {
        let mut selection = FrontierProgressSelection::new(
            ScopeId::none(),
            0,
            ScopeId::none(),
            FrontierKind::Route,
        );
        let visited = FrontierVisitSet::EMPTY;
        selection.consider(candidate(1, FrontierKind::Reentry), &visited);
        selection.consider(candidate(2, FrontierKind::PassiveObserver), &visited);

        assert_eq!(
            selection.selected().map(|candidate| candidate.entry.raw()),
            Some(1)
        );
    }
}

#[cfg(kani)]
mod kani;
