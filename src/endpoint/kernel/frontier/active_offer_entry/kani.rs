use super::{
    ActiveOfferEntry, FrontierKind, LaneOfferState, OfferEntrySummary, ScopeId, StateIndex,
};
use crate::global::const_dsl::ScopeKind;

fn frontier_from_raw(raw: u8) -> FrontierKind {
    match raw & 3 {
        0 => FrontierKind::Route,
        1 => FrontierKind::Reentry,
        2 => FrontierKind::Parallel,
        _ => FrontierKind::PassiveObserver,
    }
}

fn lane_state(frontier: FrontierKind, flags: u8) -> LaneOfferState {
    LaneOfferState {
        scope: ScopeId::new(ScopeKind::Route, 3),
        entry: StateIndex::new(7),
        parallel_root: ScopeId::new(ScopeKind::Parallel, 1),
        frontier,
        flags,
    }
}

#[kani::proof]
fn active_offer_entry_aggregation_is_exact_and_owner_stable() {
    let first_lane: u8 = kani::any();
    let first_frontier = frontier_from_raw(kani::any());
    let second_frontier = frontier_from_raw(kani::any());
    let first_flags: u8 = kani::any();
    let second_flags: u8 = kani::any();
    let first = lane_state(first_frontier, first_flags);
    let mut second = lane_state(second_frontier, second_flags);
    if kani::any() {
        second.scope = ScopeId::new(ScopeKind::Route, 4);
    }
    if kani::any() {
        second.parallel_root = ScopeId::new(ScopeKind::Parallel, 2);
    }
    let mut active = ActiveOfferEntry::new(first_lane, first).unwrap();

    assert!(active.observe_lane(second));
    assert_eq!(active.representative_lane(), first_lane);
    assert_eq!(active.representative(), first);
    assert_eq!(
        active.summary().frontier_mask,
        first_frontier.bit() | second_frontier.bit()
    );
    assert_eq!(
        active.summary().flags,
        (first_flags | second_flags)
            & (OfferEntrySummary::FLAG_CONTROLLER
                | OfferEntrySummary::FLAG_DYNAMIC
                | OfferEntrySummary::FLAG_INTRINSIC_READY)
    );
}

#[kani::proof]
fn active_offer_entry_foreign_entry_is_atomic_rejection() {
    let mut active = ActiveOfferEntry::new(2, lane_state(FrontierKind::Route, 0)).unwrap();
    let before = active;
    let mut foreign = lane_state(FrontierKind::Parallel, kani::any());
    foreign.entry = StateIndex::new(8);

    assert!(!active.observe_lane(foreign));
    assert_eq!(active, before);
}
