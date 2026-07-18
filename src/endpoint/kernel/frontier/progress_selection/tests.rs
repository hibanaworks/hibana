use super::*;

fn candidate_at(
    scope_id: ScopeId,
    entry: u16,
    parallel_root: ScopeId,
    frontier: FrontierKind,
) -> FrontierProgressCandidate {
    FrontierProgressCandidate {
        scope_id,
        entry: StateIndex::new(entry),
        parallel_root,
        frontier,
    }
}

fn candidate(entry: u16, frontier: FrontierKind) -> FrontierProgressCandidate {
    candidate_at(ScopeId::route(1), entry, ScopeId::none(), frontier)
}

#[test]
fn progress_selection_distinguishes_entries_that_share_a_scope() {
    let mut selection =
        FrontierProgressSelection::new(ScopeId::none(), 0, ScopeId::none(), FrontierKind::Route);
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
    let mut selection =
        FrontierProgressSelection::new(ScopeId::none(), 0, ScopeId::none(), FrontierKind::Route);
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
    let mut selection =
        FrontierProgressSelection::new(ScopeId::none(), 0, ScopeId::none(), FrontierKind::Route);
    let visited = FrontierVisitSet::EMPTY;
    selection.consider(candidate(1, FrontierKind::Reentry), &visited);
    selection.consider(candidate(2, FrontierKind::PassiveObserver), &visited);

    assert_eq!(
        selection.selected().map(|candidate| candidate.entry.raw()),
        Some(1)
    );
}

#[test]
fn progress_selection_filters_every_admission_condition_before_priority() {
    let current_scope = ScopeId::route(1);
    let current_root = ScopeId::parallel(2);
    let mut selection =
        FrontierProgressSelection::new(current_scope, 1, current_root, FrontierKind::Route);
    let mut visited_storage = [StateIndex::ABSENT; 1];
    /* SAFETY: `visited_storage` is initialized, live, and exclusively
    borrowed for the complete visit-set use. */
    let mut visited = unsafe {
        FrontierVisitSet::from_parts(visited_storage.as_mut_ptr(), visited_storage.len())
    };
    visited.record(4);

    selection.consider(
        candidate_at(current_scope, 1, current_root, FrontierKind::Route),
        &visited,
    );
    selection.consider(
        candidate_at(
            ScopeId::route(2),
            2,
            ScopeId::parallel(3),
            FrontierKind::Route,
        ),
        &visited,
    );
    selection.consider(
        candidate_at(ScopeId::route(2), 4, current_root, FrontierKind::Route),
        &visited,
    );
    selection.consider(
        candidate_at(ScopeId::route(2), 3, current_root, FrontierKind::Reentry),
        &visited,
    );
    selection.consider(
        candidate_at(ScopeId::route(3), 5, current_root, FrontierKind::Route),
        &visited,
    );

    assert_eq!(
        selection.selected().map(|candidate| candidate.entry.raw()),
        Some(5)
    );
}
