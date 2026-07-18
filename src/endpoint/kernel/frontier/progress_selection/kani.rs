use super::{
    FrontierKind, FrontierProgressCandidate, FrontierProgressSelection, FrontierVisitSet, ScopeId,
    StateIndex,
};

fn candidate(entry: u16, frontier: FrontierKind) -> FrontierProgressCandidate {
    FrontierProgressCandidate {
        scope_id: ScopeId::route(1),
        entry: StateIndex::new(entry),
        parallel_root: ScopeId::none(),
        frontier,
    }
}

#[kani::proof]
fn streaming_progress_selection_preserves_first_matching_frontier_priority() {
    let first_frontier: FrontierKind = kani::any();
    let mut selection =
        FrontierProgressSelection::new(ScopeId::none(), 0, ScopeId::none(), FrontierKind::Route);
    let visited = FrontierVisitSet::EMPTY;
    selection.consider(candidate(1, first_frontier), &visited);
    selection.consider(candidate(2, FrontierKind::Route), &visited);

    let expected = if first_frontier == FrontierKind::Route {
        1
    } else {
        2
    };
    assert_eq!(
        selection.selected().map(|item| item.entry.raw()),
        Some(expected)
    );
}

#[kani::proof]
fn streaming_progress_selection_retains_first_admissible_in_constant_state() {
    let first_frontier = if kani::any::<bool>() {
        FrontierKind::Reentry
    } else {
        FrontierKind::Parallel
    };
    let second_frontier = if kani::any::<bool>() {
        FrontierKind::Parallel
    } else {
        FrontierKind::PassiveObserver
    };
    let mut selection =
        FrontierProgressSelection::new(ScopeId::none(), 0, ScopeId::none(), FrontierKind::Route);
    let visited = FrontierVisitSet::EMPTY;
    selection.consider(candidate(1, first_frontier), &visited);
    selection.consider(candidate(2, second_frontier), &visited);

    assert_eq!(selection.selected().map(|item| item.entry.raw()), Some(1));
}
