use super::*;
use crate::endpoint::kernel::frontier::{
    FrontierKind, OfferEntryAdmission, OfferEntryObservedState,
};
use crate::global::const_dsl::{ScopeId, ScopeKind};
use crate::global::typestate::StateIndex;

fn offer_key(entry: u16, scope: u16) -> OfferEntryKey {
    OfferEntryKey::new(
        ScopeId::new(ScopeKind::Route, scope),
        StateIndex::new(entry),
    )
    .expect("present offer key")
}

#[test]
fn active_entry_set_accepts_the_complete_lane_domain() {
    let mut storage = [ActiveEntrySlot::EMPTY; 256];
    let mut entries = ActiveEntrySetBuilder::from_slice(&mut storage);

    for lane in 0u8..=u8::MAX {
        entries.insert_key(offer_key(lane as u16, lane as u16), lane);
    }

    assert_eq!(entries.len(), 256);
    let entries = entries.seal();
    assert_eq!(
        entries.slot_at(8).map(|slot| slot.key.entry().as_usize()),
        Some(8)
    );
    assert_eq!(
        entries.slot_at(255).map(|slot| slot.key.entry().as_usize()),
        Some(255)
    );
}

#[test]
fn active_entry_set_distinguishes_scopes_that_share_an_entry() {
    let mut storage = [ActiveEntrySlot::EMPTY; 2];
    let mut entries = ActiveEntrySetBuilder::from_slice(&mut storage);
    let first = offer_key(7, 1);
    let second = offer_key(7, 2);

    entries.insert_key(first, 0);
    entries.insert_key(second, 1);
    assert_eq!(entries.len(), 2);
    let entries = entries.seal();
    assert_eq!(entries.slot_at(0).map(|slot| slot.key), Some(first));
    assert_eq!(entries.slot_at(1).map(|slot| slot.key), Some(second));
    assert_eq!(core::mem::size_of::<ActiveEntrySlot>(), 6);
}

#[test]
#[should_panic]
fn active_entry_set_rejects_an_absent_key() {
    let mut storage = [ActiveEntrySlot::EMPTY; 1];
    let mut entries = ActiveEntrySetBuilder::from_slice(&mut storage);
    entries.insert_key(OfferEntryKey::EMPTY, 0);
}

#[test]
fn offer_entry_key_accepts_only_route_scopes() {
    let entry = StateIndex::new(7);
    assert!(OfferEntryKey::new(ScopeId::route(1), entry).is_some());
    assert!(OfferEntryKey::new(ScopeId::parallel(1), entry).is_none());
    assert!(OfferEntryKey::new(ScopeId::roll_scope(1), entry).is_none());
    assert!(OfferEntryKey::new(ScopeId::none(), entry).is_none());
}

#[test]
fn observed_entry_set_streams_the_full_lane_domain() {
    let mut storage = [FrontierObservationSlot::EMPTY; 256];
    let mut observed = ObservedEntrySetBuilder::from_slice(&mut storage);
    observed.clear();

    for entry_idx in 0..256 {
        let slot_idx = observed.push_exact_observation(
            OfferEntryObservedState {
                key: offer_key(entry_idx as u16, 1),
                frontier_mask: FrontierKind::Route.bit(),
                flags: OfferEntryObservedState::FLAG_READY,
            },
            OfferEntryAdmission::Selectable,
        );
        assert_eq!(slot_idx, entry_idx);
    }

    let observed = observed.seal();
    assert_eq!(observed.len(), 256);
    assert_eq!(observed.entry_idx(8), Some(8));
    assert_eq!(observed.entry_idx(255), Some(255));
    assert!(observed.slot(255).is_some_and(|slot| {
        slot.is_selectable() && slot.is_ready() && slot.is_in_frontier(FrontierKind::Route)
    }));
}

#[test]
fn observed_entry_set_preserves_each_exact_scope_witness() {
    let mut storage = [FrontierObservationSlot::EMPTY; 2];
    let mut observed = ObservedEntrySetBuilder::from_slice(&mut storage);
    observed.clear();
    observed.push_exact_observation(
        OfferEntryObservedState {
            key: offer_key(7, 1),
            frontier_mask: FrontierKind::Route.bit(),
            flags: OfferEntryObservedState::FLAG_CONTROLLER,
        },
        OfferEntryAdmission::Selectable,
    );
    observed.push_exact_observation(
        OfferEntryObservedState {
            key: offer_key(7, 2),
            frontier_mask: FrontierKind::Reentry.bit(),
            flags: OfferEntryObservedState::FLAG_PROGRESS | OfferEntryObservedState::FLAG_READY,
        },
        OfferEntryAdmission::Selectable,
    );

    let observed = observed.seal();
    assert_eq!(observed.len(), 2);
    assert_eq!(observed.entry_group_end(0), Some(2));
    let controller = observed.slot(0).expect("controller witness");
    let progress = observed.slot(1).expect("progress witness");
    assert!(controller.is_controller());
    assert!(!controller.has_progress());
    assert!(controller.is_in_frontier(FrontierKind::Route));
    assert!(!controller.is_in_frontier(FrontierKind::Reentry));
    assert!(!progress.is_controller());
    assert!(progress.has_progress());
    assert!(!progress.is_in_frontier(FrontierKind::Route));
    assert!(progress.is_in_frontier(FrontierKind::Reentry));
}

#[test]
fn exact_observations_group_equal_entries_after_out_of_order_insertion() {
    let mut storage = [FrontierObservationSlot::EMPTY; 4];
    let mut observed = ObservedEntrySetBuilder::from_slice(&mut storage);
    observed.clear();
    for (entry, scope) in [(9, 1), (7, 2), (8, 3), (7, 4)] {
        observed.push_exact_observation(
            OfferEntryObservedState {
                key: offer_key(entry, scope),
                frontier_mask: FrontierKind::Route.bit(),
                flags: 0,
            },
            OfferEntryAdmission::Selectable,
        );
    }

    let observed = observed.seal();
    assert_eq!(observed.entry_idx(0), Some(7));
    assert_eq!(observed.entry_idx(1), Some(7));
    assert_eq!(observed.entry_group_end(0), Some(2));
    assert_eq!(observed.entry_idx(2), Some(8));
    assert_eq!(observed.entry_idx(3), Some(9));
}

#[test]
fn selectable_ready_query_ignores_excluded_exact_witnesses() {
    let mut storage = [FrontierObservationSlot::EMPTY; 3];
    let mut observed = ObservedEntrySetBuilder::from_slice(&mut storage);
    observed.clear();
    observed.push_exact_observation(
        OfferEntryObservedState {
            key: offer_key(7, 1),
            frontier_mask: FrontierKind::Reentry.bit(),
            flags: OfferEntryObservedState::FLAG_READY,
        },
        OfferEntryAdmission::Excluded,
    );
    observed.push_exact_observation(
        OfferEntryObservedState {
            key: offer_key(8, 2),
            frontier_mask: FrontierKind::Reentry.bit(),
            flags: 0,
        },
        OfferEntryAdmission::Selectable,
    );
    observed.push_exact_observation(
        OfferEntryObservedState {
            key: offer_key(9, 3),
            frontier_mask: FrontierKind::Reentry.bit(),
            flags: OfferEntryObservedState::FLAG_READY,
        },
        OfferEntryAdmission::Selectable,
    );

    let observed = observed.seal();
    assert_eq!(observed.first_selectable_ready_entry_except(0), Some(9));
    assert_eq!(observed.first_selectable_ready_entry_except(9), None);
}

#[test]
#[should_panic]
fn exact_observation_capacity_exhaustion_is_an_invariant_failure() {
    let mut storage = [FrontierObservationSlot::EMPTY; 1];
    let mut observed = ObservedEntrySetBuilder::from_slice(&mut storage);
    observed.clear();
    for entry in [7, 8] {
        observed.push_exact_observation(
            OfferEntryObservedState {
                key: offer_key(entry, 1),
                frontier_mask: FrontierKind::Route.bit(),
                flags: 0,
            },
            OfferEntryAdmission::Selectable,
        );
    }
}

#[test]
#[should_panic]
fn observation_rejects_an_absent_exact_key() {
    let _ = FrontierObservationSlot::from_exact_observation(
        OfferEntryObservedState {
            key: OfferEntryKey::EMPTY,
            frontier_mask: FrontierKind::Route.bit(),
            flags: 0,
        },
        OfferEntryAdmission::Excluded,
    );
}
