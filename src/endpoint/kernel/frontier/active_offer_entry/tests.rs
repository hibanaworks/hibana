use super::*;
use crate::global::const_dsl::ScopeKind;

fn lane_state(
    entry: u16,
    scope: ScopeId,
    parallel_root: ScopeId,
    frontier: FrontierKind,
    flags: u8,
) -> LaneOfferState {
    LaneOfferState {
        scope,
        entry: StateIndex::new(entry),
        parallel_root,
        frontier,
        flags,
    }
}

#[test]
fn active_offer_entry_keeps_one_exact_owner_and_aggregates_all_lanes() {
    let scope = ScopeId::new(ScopeKind::Route, 3);
    let root = ScopeId::new(ScopeKind::Parallel, 1);
    let first = lane_state(
        7,
        scope,
        root,
        FrontierKind::Route,
        LaneOfferState::FLAG_CONTROLLER,
    );
    let second = lane_state(
        7,
        ScopeId::new(ScopeKind::Route, 4),
        ScopeId::new(ScopeKind::Parallel, 2),
        FrontierKind::Reentry,
        LaneOfferState::FLAG_DYNAMIC | LaneOfferState::FLAG_INTRINSIC_READY,
    );
    let mut active = ActiveOfferEntry::new(2, first).expect("valid active owner");

    assert!(active.observe_lane(second));
    assert_eq!(active.representative_lane(), 2);
    assert_eq!(active.representative(), first);
    assert_eq!(
        active.summary().frontier_mask,
        FrontierKind::Route.bit() | FrontierKind::Reentry.bit()
    );
    assert!(active.summary().is_controller());
    assert!(active.summary().is_dynamic());
    assert!(active.summary().intrinsic_ready());
}

#[test]
fn active_offer_entry_rejects_foreign_entry_without_partial_update() {
    let scope = ScopeId::new(ScopeKind::Route, 3);
    let first = lane_state(
        7,
        scope,
        ScopeId::none(),
        FrontierKind::Route,
        LaneOfferState::FLAG_CONTROLLER,
    );
    let mut active = ActiveOfferEntry::new(2, first).expect("valid active owner");
    let before = active;
    let foreign = lane_state(
        8,
        scope,
        ScopeId::none(),
        FrontierKind::Parallel,
        LaneOfferState::FLAG_DYNAMIC,
    );

    assert!(!active.observe_lane(foreign));
    assert_eq!(active, before);
    assert!(ActiveOfferEntry::new(2, LaneOfferState::EMPTY).is_none());
}
