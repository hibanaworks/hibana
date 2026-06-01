use crate::endpoint::kernel::core::offer_regression_tests::cases::*;
#[test]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn frontier_yield_ping_pong_is_bounded()
 {
    let mut visited_slots = frontier_visit_slots::<2>();
    let mut visited = frontier_visit_set_fixture(&mut visited_slots);
    let scope_a = ScopeId::generic(31);
    let scope_b = ScopeId::generic(32);
    visited.record(scope_a);
    visited.record(scope_b);
    visited.record(scope_a);
    assert!(visited.contains(scope_a));
    assert!(visited.contains(scope_b));
    assert_eq!(visited.len, 2);
}

#[test]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn route_defer_yields_to_sibling_scope()
 {
    let current_scope = ScopeId::generic(41);
    let sibling_scope = ScopeId::generic(42);
    let mut candidates = frontier_candidates::<2>();
    candidates[0] = FrontierCandidate {
        scope_id: current_scope,
        entry_idx: 10,
        parallel_root: ScopeId::none(),
        frontier: FrontierKind::Route,
        flags: FrontierCandidate::pack_flags(true, true, false, false),
    };
    candidates[1] = FrontierCandidate {
        scope_id: sibling_scope,
        entry_idx: 12,
        parallel_root: ScopeId::none(),
        frontier: FrontierKind::Route,
        flags: FrontierCandidate::pack_flags(true, true, true, true),
    };
    let snapshot = frontier_snapshot_fixture(
        current_scope,
        10,
        ScopeId::none(),
        FrontierKind::Route,
        &mut candidates,
        2,
    );
    let picked = snapshot
        .select_yield_candidate(empty_frontier_visit_set())
        .expect("route frontier must yield to progress sibling");
    assert_eq!(picked.scope_id, sibling_scope);
    assert_eq!(picked.frontier, FrontierKind::Route);
}

#[test]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn loop_defer_yields_to_sibling_scope()
 {
    let current_scope = ScopeId::generic(51);
    let sibling_scope = ScopeId::generic(52);
    let mut candidates = frontier_candidates::<2>();
    candidates[0] = FrontierCandidate {
        scope_id: current_scope,
        entry_idx: 20,
        parallel_root: ScopeId::none(),
        frontier: FrontierKind::Loop,
        flags: FrontierCandidate::pack_flags(true, true, false, false),
    };
    candidates[1] = FrontierCandidate {
        scope_id: sibling_scope,
        entry_idx: 24,
        parallel_root: ScopeId::none(),
        frontier: FrontierKind::Loop,
        flags: FrontierCandidate::pack_flags(true, true, true, true),
    };
    let snapshot = frontier_snapshot_fixture(
        current_scope,
        20,
        ScopeId::none(),
        FrontierKind::Loop,
        &mut candidates,
        2,
    );
    let picked = snapshot
        .select_yield_candidate(empty_frontier_visit_set())
        .expect("loop frontier must yield to progress sibling");
    assert_eq!(picked.scope_id, sibling_scope);
    assert_eq!(picked.frontier, FrontierKind::Loop);
}

#[test]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn defer_yields_across_frontier_in_same_parallel_root()
 {
    let root = ScopeId::generic(55);
    let current_scope = ScopeId::generic(56);
    let sibling_scope = ScopeId::generic(57);
    let mut candidates = frontier_candidates::<3>();
    candidates[0] = FrontierCandidate {
        scope_id: current_scope,
        entry_idx: 20,
        parallel_root: root,
        frontier: FrontierKind::Loop,
        flags: FrontierCandidate::pack_flags(true, true, false, false),
    };
    candidates[1] = FrontierCandidate {
        scope_id: sibling_scope,
        entry_idx: 24,
        parallel_root: root,
        frontier: FrontierKind::Route,
        flags: FrontierCandidate::pack_flags(true, true, true, true),
    };
    let snapshot = frontier_snapshot_fixture(
        current_scope,
        20,
        root,
        FrontierKind::Loop,
        &mut candidates,
        2,
    );
    let picked = snapshot
        .select_yield_candidate(empty_frontier_visit_set())
        .expect("defer must yield to progress sibling in same parallel root");
    assert_eq!(picked.scope_id, sibling_scope);
    assert_eq!(picked.frontier, FrontierKind::Route);
}

#[test]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn parallel_frontier_prefers_ready_lane_before_phase_join()
 {
    let current_scope = ScopeId::generic(61);
    let root = ScopeId::generic(60);
    let ready_scope = ScopeId::generic(62);
    let mut candidates = frontier_candidates::<3>();
    candidates[0] = FrontierCandidate {
        scope_id: current_scope,
        entry_idx: 30,
        parallel_root: root,
        frontier: FrontierKind::Parallel,
        flags: FrontierCandidate::pack_flags(true, true, false, false),
    };
    candidates[1] = FrontierCandidate {
        scope_id: ScopeId::generic(63),
        entry_idx: 31,
        parallel_root: root,
        frontier: FrontierKind::Parallel,
        flags: FrontierCandidate::pack_flags(false, false, false, false),
    };
    candidates[2] = FrontierCandidate {
        scope_id: ready_scope,
        entry_idx: 32,
        parallel_root: root,
        frontier: FrontierKind::Parallel,
        flags: FrontierCandidate::pack_flags(false, false, true, true),
    };
    let snapshot = frontier_snapshot_fixture(
        current_scope,
        30,
        root,
        FrontierKind::Parallel,
        &mut candidates,
        3,
    );
    let picked = snapshot
        .select_yield_candidate(empty_frontier_visit_set())
        .expect("parallel frontier must choose progress sibling");
    assert_eq!(picked.scope_id, ready_scope);
    assert_eq!(picked.entry_idx, 32);
}

#[test]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn passive_observer_defer_follow_is_progressive()
 {
    let current_scope = ScopeId::generic(71);
    let sibling_scope = ScopeId::generic(72);
    let mut candidates = frontier_candidates::<2>();
    candidates[0] = FrontierCandidate {
        scope_id: current_scope,
        entry_idx: 40,
        parallel_root: ScopeId::none(),
        frontier: FrontierKind::PassiveObserver,
        flags: FrontierCandidate::pack_flags(false, false, false, true),
    };
    candidates[1] = FrontierCandidate {
        scope_id: sibling_scope,
        entry_idx: 44,
        parallel_root: ScopeId::none(),
        frontier: FrontierKind::PassiveObserver,
        flags: FrontierCandidate::pack_flags(false, false, true, true),
    };
    let snapshot = frontier_snapshot_fixture(
        current_scope,
        40,
        ScopeId::none(),
        FrontierKind::PassiveObserver,
        &mut candidates,
        2,
    );
    let mut visited_slots = frontier_visit_slots::<1>();
    let mut visited = frontier_visit_set_fixture(&mut visited_slots);
    visited.record(current_scope);
    let picked = snapshot
        .select_yield_candidate(visited)
        .expect("passive observer defer must progress to sibling");
    assert_eq!(picked.scope_id, sibling_scope);
    assert_ne!(picked.scope_id, current_scope);
}

#[test]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn passive_observer_defer_stops_without_progress_evidence()
 {
    let root = ScopeId::generic(73);
    let current_scope = ScopeId::generic(74);
    let sibling_scope = ScopeId::generic(75);
    let mut candidates = frontier_candidates::<2>();
    candidates[0] = FrontierCandidate {
        scope_id: current_scope,
        entry_idx: 50,
        parallel_root: root,
        frontier: FrontierKind::PassiveObserver,
        flags: FrontierCandidate::pack_flags(false, false, false, true),
    };
    candidates[1] = FrontierCandidate {
        scope_id: sibling_scope,
        entry_idx: 53,
        parallel_root: root,
        frontier: FrontierKind::Loop,
        flags: FrontierCandidate::pack_flags(true, false, false, true),
    };
    let snapshot = frontier_snapshot_fixture(
        current_scope,
        50,
        root,
        FrontierKind::PassiveObserver,
        &mut candidates,
        2,
    );
    let mut visited_slots = frontier_visit_slots::<1>();
    let mut visited = frontier_visit_set_fixture(&mut visited_slots);
    visited.record(current_scope);
    assert_eq!(snapshot.select_yield_candidate(visited), None);
}
