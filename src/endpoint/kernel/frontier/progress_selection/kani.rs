use super::{
    FrontierKind, FrontierProgressCandidate, FrontierProgressSelection, FrontierVisitSet, ScopeId,
    StateIndex,
};

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

#[kani::proof]
fn streaming_progress_selection_matches_the_exact_filtered_reference() {
    let current_scope = ScopeId::route(1);
    let current_entry = StateIndex::new(1);
    let current_parallel_root = if kani::any() {
        ScopeId::none()
    } else {
        ScopeId::parallel(1)
    };
    let current_frontier: FrontierKind = kani::any();
    let first = candidate_at(
        if kani::any() {
            current_scope
        } else {
            ScopeId::route(2)
        },
        if kani::any() { current_entry.raw() } else { 2 },
        if kani::any() {
            current_parallel_root
        } else {
            ScopeId::parallel(2)
        },
        kani::any(),
    );
    let second = candidate_at(
        if kani::any() {
            current_scope
        } else {
            ScopeId::route(3)
        },
        if kani::any() { current_entry.raw() } else { 3 },
        if kani::any() {
            current_parallel_root
        } else {
            ScopeId::parallel(3)
        },
        kani::any(),
    );
    let visit_first: bool = kani::any();
    let visit_second: bool = kani::any();
    let mut visited_storage = [StateIndex::ABSENT; 2];
    /* SAFETY: `visited_storage` is initialized, live, and exclusively borrowed
    for the complete symbolic visit-set use. */
    let mut visited = unsafe {
        FrontierVisitSet::from_parts(visited_storage.as_mut_ptr(), visited_storage.len())
    };
    if visit_first {
        visited.record(first.entry.as_usize());
    }
    if visit_second {
        visited.record(second.entry.as_usize());
    }

    let exact_admissible = |candidate: FrontierProgressCandidate| {
        (candidate.scope_id != current_scope || candidate.entry != current_entry)
            && (current_parallel_root.is_none() || candidate.parallel_root == current_parallel_root)
            && !((visit_first && candidate.entry == first.entry)
                || (visit_second && candidate.entry == second.entry))
    };
    let first_admissible = exact_admissible(first);
    let second_admissible = exact_admissible(second);
    let expected_preferred = if first_admissible && first.frontier == current_frontier {
        Some(first)
    } else if second_admissible && second.frontier == current_frontier {
        Some(second)
    } else {
        None
    };
    let expected_first_admissible = if first_admissible {
        Some(first)
    } else if second_admissible {
        Some(second)
    } else {
        None
    };
    let expected = match expected_preferred {
        Some(preferred) => Some(preferred),
        None => expected_first_admissible,
    };
    let mut selection = FrontierProgressSelection::new(
        current_scope,
        current_entry.as_usize(),
        current_parallel_root,
        current_frontier,
    );

    assert_eq!(
        selection.candidate_is_admissible(first, &visited),
        first_admissible
    );
    assert_eq!(
        selection.candidate_is_admissible(second, &visited),
        second_admissible
    );
    selection.consider(first, &visited);
    selection.consider(second, &visited);

    assert_eq!(selection.selected(), expected);
}
