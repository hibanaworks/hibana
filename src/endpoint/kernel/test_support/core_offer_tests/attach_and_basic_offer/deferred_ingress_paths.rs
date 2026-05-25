use crate::endpoint::kernel::core::offer_regression_tests::cases::*;
#[test]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn active_entry_set_orders_entries_by_representative_lane()
 {
    let (_entry_slots, mut entries) = active_entry_set_storage(3);
    assert!(entries.insert_entry(9, 4));
    assert!(entries.insert_entry(3, 1));
    assert!(entries.insert_entry(7, 1));
    assert_eq!(entries.entry_at(0), Some(3));
    assert_eq!(entries.entry_at(1), Some(7));
    assert_eq!(entries.entry_at(2), Some(9));

    assert!(entries.remove_entry(3));
    assert_eq!(entries.entry_at(0), Some(7));
    assert_eq!(entries.entry_at(1), Some(9));
    assert_eq!(entries.occupancy_mask(), 0b0000_0011);
}

#[test]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn current_passive_without_evidence_keeps_priority_with_controller_present()
 {
    assert!(!current_entry_is_candidate(false, false, false, 0, false,));
    assert!(current_entry_is_candidate(true, false, false, 1, false,));
}

#[test]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn current_passive_with_evidence_keeps_priority()
 {
    assert!(current_entry_is_candidate(true, false, true, 1, false,));
}

#[test]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn current_passive_without_controller_keeps_priority()
 {
    assert!(current_entry_is_candidate(true, false, false, 1, false,));
}

#[test]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn current_passive_observer_without_evidence_keeps_priority()
 {
    assert!(current_entry_is_candidate(true, false, false, 1, false,));
}

#[test]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn current_candidate_stays_selectable_without_route_lane_metadata()
 {
    assert!(current_entry_matches_after_filter(true, true, 43, None));
}

#[test]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn current_candidate_respects_hint_filter()
 {
    assert!(!current_entry_matches_after_filter(
        true,
        true,
        43,
        Some(47)
    ));
}

#[test]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn current_without_candidate_stays_blocked()
 {
    assert!(!current_entry_matches_after_filter(false, true, 43, None));
}

#[test]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn current_without_offer_lanes_stays_blocked()
 {
    assert!(!current_entry_matches_after_filter(true, false, 43, None));
}

#[test]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn offer_entry_observed_state_merges_static_summary_and_dynamic_evidence()
 {
    let mut summary = OfferEntryStaticSummary::EMPTY;
    summary.observe_lane(LaneOfferState {
        frontier: FrontierKind::Parallel,
        flags: LaneOfferState::FLAG_CONTROLLER,
        ..LaneOfferState::EMPTY
    });
    summary.observe_lane(LaneOfferState {
        frontier: FrontierKind::Parallel,
        static_ready: true,
        flags: LaneOfferState::FLAG_DYNAMIC,
        ..LaneOfferState::EMPTY
    });
    let observed = offer_entry_observed_state(ScopeId::generic(41), summary, true, false, true);

    assert_eq!(observed.scope_id, ScopeId::generic(41));
    assert!(observed.matches_frontier(FrontierKind::Parallel));
    assert!(observed.is_controller());
    assert!(observed.is_dynamic());
    assert!(observed.has_progress_evidence());
    assert!(observed.has_ready_arm_evidence());
    assert!(observed.binding_ready());
    assert_ne!(observed.flags & OfferEntryObservedState::FLAG_READY, 0);
}

#[test]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn cached_offer_entry_observed_state_preserves_arbitration_bits()
 {
    let mut summary = OfferEntryStaticSummary::EMPTY;
    summary.observe_lane(LaneOfferState {
        frontier: FrontierKind::PassiveObserver,
        flags: LaneOfferState::FLAG_CONTROLLER | LaneOfferState::FLAG_DYNAMIC,
        ..LaneOfferState::EMPTY
    });
    let observed = offer_entry_observed_state(ScopeId::generic(51), summary, true, false, true);
    let (_observed_slots, mut observed_entries) = observed_entry_set_storage(1);
    let (observed_bit, inserted) = observed_entries.insert_entry(17).expect("insert entry");
    assert!(inserted);
    observed_entries.observe(observed_bit, observed);

    let cached = cached_offer_entry_observed_state(
        ScopeId::generic(51),
        summary,
        observed_entries,
        observed_bit,
    );
    let original_candidate = offer_entry_frontier_candidate(
        ScopeId::generic(51),
        17,
        ScopeId::generic(9),
        FrontierKind::PassiveObserver,
        observed,
    );
    let cached_candidate = offer_entry_frontier_candidate(
        ScopeId::generic(51),
        17,
        ScopeId::generic(9),
        FrontierKind::PassiveObserver,
        cached,
    );

    assert!(cached.matches_frontier(FrontierKind::PassiveObserver));
    assert!(cached.is_controller());
    assert!(cached.is_dynamic());
    assert!(cached.has_progress_evidence());
    assert!(cached.has_ready_arm_evidence());
    assert!(cached.ready());
    assert_eq!(cached_candidate.scope_id, original_candidate.scope_id);
    assert_eq!(
        cached_candidate.parallel_root,
        original_candidate.parallel_root
    );
    assert_eq!(cached_candidate.frontier, original_candidate.frontier);
    assert_eq!(
        cached_candidate.is_controller(),
        original_candidate.is_controller()
    );
    assert_eq!(
        cached_candidate.is_dynamic(),
        original_candidate.is_dynamic()
    );
    assert_eq!(
        cached_candidate.has_evidence(),
        original_candidate.has_evidence()
    );
    assert_eq!(cached_candidate.ready(), original_candidate.ready());
}

#[test]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn observed_entry_set_entry_bit_tracks_inserted_entries_exactly()
 {
    let (_observed_slots, mut observed_entries) = observed_entry_set_storage(2);
    let (first_bit, inserted_first) = observed_entries.insert_entry(17).expect("insert first");
    assert!(inserted_first);
    let (second_bit, inserted_second) = observed_entries.insert_entry(3).expect("insert second");
    assert!(inserted_second);
    let (reused_bit, inserted_reused) = observed_entries.insert_entry(17).expect("reuse first");
    assert!(!inserted_reused);
    assert_eq!(reused_bit, first_bit);
    assert_eq!(observed_entries.entry_bit(17), first_bit);
    assert_eq!(observed_entries.entry_bit(3), second_bit);
    assert_eq!(observed_entries.entry_bit(9), 0);
}
