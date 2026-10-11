use super::*;
use crate::global::const_dsl::ScopeKind;
use crate::global::typestate::StateIndex;

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
fn active_offer_entry_accepts_only_identical_scope_entry_metadata() {
    let scope = ScopeId::new(ScopeKind::Route, 3);
    let root = ScopeId::new(ScopeKind::Parallel, 1);
    let first = lane_state(
        7,
        scope,
        root,
        FrontierKind::Route,
        LaneOfferState::FLAG_CONTROLLER,
    );
    let active = ActiveOfferEntry::new(2, first).expect("valid active owner");

    assert!(active.accepts_lane(first));
    assert_eq!(active.representative_lane(), 2);
    assert_eq!(active.representative(), first);
    assert_eq!(
        active.representative().key(),
        Some(first.key().expect("exact offer key"))
    );
}

#[test]
fn active_offer_entry_rejects_foreign_scope_even_when_entry_matches() {
    let scope = ScopeId::new(ScopeKind::Route, 3);
    let first = lane_state(
        7,
        scope,
        ScopeId::none(),
        FrontierKind::Route,
        LaneOfferState::FLAG_CONTROLLER,
    );
    let active = ActiveOfferEntry::new(2, first).expect("valid active owner");
    let foreign = lane_state(
        7,
        ScopeId::new(ScopeKind::Route, 4),
        ScopeId::none(),
        FrontierKind::Parallel,
        LaneOfferState::FLAG_DYNAMIC,
    );

    assert!(!active.accepts_lane(foreign));
    assert!(ActiveOfferEntry::new(2, LaneOfferState::EMPTY).is_none());
}
