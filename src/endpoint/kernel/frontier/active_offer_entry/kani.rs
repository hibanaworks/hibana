use super::{ActiveOfferEntry, FrontierKind, LaneOfferState, ScopeId};
use crate::global::const_dsl::ScopeKind;
use crate::global::typestate::StateIndex;

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
fn active_offer_entry_accepts_only_exact_scope_entry_metadata() {
    let first_lane: u8 = kani::any();
    let first_frontier = frontier_from_raw(kani::any());
    let first_flags: u8 = kani::any();
    let first = lane_state(first_frontier, first_flags);
    let active = ActiveOfferEntry::new(first_lane, first).unwrap();

    assert!(active.accepts_lane(first));
    assert_eq!(active.representative_lane(), first_lane);
    assert_eq!(active.representative(), first);
}

#[kani::proof]
fn active_offer_entry_foreign_scope_is_exact_rejection() {
    let active = ActiveOfferEntry::new(2, lane_state(FrontierKind::Route, 0)).unwrap();
    let mut foreign = lane_state(FrontierKind::Parallel, kani::any());
    foreign.scope = ScopeId::new(ScopeKind::Route, 4);

    assert!(!active.accepts_lane(foreign));
    assert_ne!(active.representative().key(), foreign.key());
}
