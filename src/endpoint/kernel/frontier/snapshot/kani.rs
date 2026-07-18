use super::{
    FrontierCandidate, FrontierKind, FrontierSnapshot, FrontierVisitSet, ScopeId, StateIndex,
    checked_state_index,
};
use crate::global::typestate::MAX_STATES;

#[kani::proof]
fn visited_entry_identity_is_exact_and_never_silent() {
    let first_candidate = kani::any::<u8>();
    let second_candidate = kani::any::<u8>();
    let second_candidate = if first_candidate == second_candidate {
        first_candidate.wrapping_add(1)
    } else {
        second_candidate
    };
    let first = first_candidate as usize;
    let second = second_candidate as usize;
    let mut storage = [StateIndex::ABSENT; 2];
    /* SAFETY: `storage` is initialized, live, and exclusively borrowed for the
    complete symbolic visit-set execution. */
    let mut visited = unsafe { FrontierVisitSet::from_parts(storage.as_mut_ptr(), storage.len()) };

    visited.record(first);
    visited.record(second);

    assert_eq!(visited.len(), 2);
    assert!(visited.contains(first));
    assert!(visited.contains(second));
}

#[kani::proof]
fn absent_state_identity_is_rejected() {
    assert!(checked_state_index(MAX_STATES - 1).is_some());
    assert!(checked_state_index(MAX_STATES).is_none());
}

#[kani::proof]
fn repeated_alignment_source_remains_detectable_without_capacity_growth() {
    let source = kani::any::<u8>() as usize;
    let mut storage = [StateIndex::ABSENT; 1];
    /* SAFETY: `storage` is initialized, live, and exclusively borrowed for the
    complete symbolic visit-set execution. */
    let mut visited = unsafe { FrontierVisitSet::from_parts(storage.as_mut_ptr(), storage.len()) };

    visited.record(source);
    assert!(visited.contains(source));
    visited.record(source);

    assert_eq!(visited.len(), 1);
    assert!(visited.contains(source));
}

fn candidate() -> FrontierCandidate {
    FrontierCandidate {
        scope_id: ScopeId::route(1),
        entry: StateIndex::new(1),
        parallel_root: ScopeId::none(),
        frontier: FrontierKind::Route,
        flags: FrontierCandidate::FLAG_HAS_EVIDENCE | FrontierCandidate::FLAG_READY,
    }
}

#[kani::proof]
fn two_cell_frontier_snapshot_never_publishes_a_third_candidate() {
    let mut storage = [FrontierCandidate::EMPTY; 2];
    let mut snapshot = FrontierSnapshot::new(
        ScopeId::none(),
        StateIndex::new(0),
        ScopeId::none(),
        FrontierKind::Route,
        &mut storage,
    );
    assert!(snapshot.push_candidate(candidate()));
    assert!(snapshot.push_candidate(candidate()));
    assert!(!snapshot.push_candidate(candidate()));
    assert_eq!(snapshot.candidate_len, 2);
}
