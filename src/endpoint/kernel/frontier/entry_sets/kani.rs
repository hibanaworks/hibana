use super::{ActiveEntrySetBuilder, ActiveEntrySlot, FrontierObservationSlot};
use crate::endpoint::kernel::frontier::{
    FrontierKind, OfferEntryAdmission, OfferEntryKey, OfferEntryObservedState,
};

fn offer_key(entry: u16, scope: u16) -> OfferEntryKey {
    OfferEntryKey::new(
        ScopeId::new(ScopeKind::Route, scope),
        StateIndex::new(entry),
    )
    .unwrap()
}

#[kani::proof]
fn frontier_entry_identity_distinguishes_scope_at_same_entry() {
    let mut storage = [ActiveEntrySlot::EMPTY; 2];
    let mut entries = ActiveEntrySetBuilder::from_slice(&mut storage);
    let first = offer_key(7, 1);
    let second = offer_key(7, 2);

    entries.insert_key(first, 0);
    entries.insert_key(second, 1);
    assert_eq!(entries.len(), 2);
    let entries = entries.seal();
    assert_eq!(entries.slot_at(0).unwrap().key, first);
    assert_eq!(entries.slot_at(1).unwrap().key, second);
}

#[kani::proof]
#[kani::should_panic]
fn active_frontier_entry_rejects_absent_exact_key() {
    let mut storage = [ActiveEntrySlot::EMPTY; 1];
    let mut entries = ActiveEntrySetBuilder::from_slice(&mut storage);
    entries.insert_key(OfferEntryKey::EMPTY, 0);
}

#[kani::proof]
fn offer_entry_key_rejects_non_route_scopes() {
    let entry = StateIndex::new(7);
    assert!(OfferEntryKey::new(ScopeId::route(1), entry).is_some());
    assert!(OfferEntryKey::new(ScopeId::parallel(1), entry).is_none());
    assert!(OfferEntryKey::new(ScopeId::roll_scope(1), entry).is_none());
    assert!(OfferEntryKey::new(ScopeId::none(), entry).is_none());
}
use crate::global::{
    const_dsl::{ScopeId, ScopeKind},
    role_program::RuntimeRoleFootprint,
    typestate::StateIndex,
};

#[kani::proof]
fn frontier_entry_capacity_preserves_the_full_lane_domain() {
    let mut storage = [ActiveEntrySlot::EMPTY; 256];
    let candidate: u16 = kani::any();
    let active_lane_count = if candidate as usize <= storage.len() {
        candidate
    } else {
        storage.len() as u16
    };
    let footprint = RuntimeRoleFootprint {
        max_route_commit_count: 0,
        route_arm_state_capacity: 0,
        local_step_count: 0,
        route_scope_count: 0,
        active_lane_count: active_lane_count as usize,
        endpoint_lane_slot_count: 1,
        logical_lane_count: 1,
    };
    let capacity = footprint.frontier_entry_count();
    let entries = ActiveEntrySetBuilder::from_slice(&mut storage[..capacity]);

    assert_eq!(entries.capacity(), capacity);
    assert_eq!(entries.capacity(), active_lane_count as usize);
    assert_eq!(entries.len(), 0);
    let entries = entries.seal();
    assert_eq!(entries.len(), 0);
    assert_eq!(capacity, active_lane_count as usize);
    if active_lane_count == 0 {
        assert_eq!(footprint.frontier_visit_count(), 0);
    } else {
        assert_eq!(
            footprint.frontier_visit_count(),
            active_lane_count as usize + 1
        );
        assert!(footprint.frontier_visit_count() > capacity);
    }
}

#[kani::proof]
fn frontier_observation_packing_is_exact() {
    let raw_flags = kani::any::<u8>() & OfferEntryObservedState::ALL_FLAGS;
    let raw_frontier = kani::any::<u8>() & FrontierKind::ALL_BITS;
    let selectable = kani::any::<bool>();
    let admission = if selectable {
        OfferEntryAdmission::Selectable
    } else {
        OfferEntryAdmission::Excluded
    };
    let observed = OfferEntryObservedState {
        key: offer_key(0, 1),
        frontier_mask: raw_frontier,
        flags: raw_flags,
    };
    let slot = FrontierObservationSlot::from_exact_observation(observed, admission);

    assert_eq!(slot.is_controller(), observed.is_controller());
    assert_eq!(slot.is_dynamic(), observed.is_dynamic());
    assert_eq!(slot.has_progress(), observed.has_progress_evidence());
    assert_eq!(slot.has_ready_arm(), observed.has_ready_arm_evidence());
    assert_eq!(slot.is_ready(), observed.is_ready());
    assert_eq!(slot.is_selectable(), selectable);
    for frontier in [
        FrontierKind::Route,
        FrontierKind::Parallel,
        FrontierKind::Reentry,
        FrontierKind::PassiveObserver,
    ] {
        assert_eq!(
            slot.is_in_frontier(frontier),
            (raw_frontier & frontier.bit()) != 0
        );
    }
}

#[kani::proof]
fn frontier_observation_rows_preserve_exact_witnesses_for_one_cursor_target() {
    let first_flags = kani::any::<u8>() & OfferEntryObservedState::ALL_FLAGS;
    let second_flags = kani::any::<u8>() & OfferEntryObservedState::ALL_FLAGS;
    let first_frontier = kani::any::<u8>() & FrontierKind::ALL_BITS;
    let second_frontier = kani::any::<u8>() & FrontierKind::ALL_BITS;
    let first = OfferEntryObservedState {
        key: offer_key(7, 1),
        frontier_mask: first_frontier,
        flags: first_flags,
    };
    let second = OfferEntryObservedState {
        key: offer_key(7, 2),
        frontier_mask: second_frontier,
        flags: second_flags,
    };
    let first_slot =
        FrontierObservationSlot::from_exact_observation(first, OfferEntryAdmission::Excluded);
    let second_slot =
        FrontierObservationSlot::from_exact_observation(second, OfferEntryAdmission::Selectable);

    assert_eq!(first_slot.is_controller(), first.is_controller());
    assert_eq!(first_slot.is_dynamic(), first.is_dynamic());
    assert_eq!(first_slot.has_progress(), first.has_progress_evidence());
    assert_eq!(first_slot.is_ready(), first.is_ready());
    assert!(!first_slot.is_selectable());
    assert_eq!(second_slot.is_controller(), second.is_controller());
    assert_eq!(second_slot.is_dynamic(), second.is_dynamic());
    assert_eq!(second_slot.has_progress(), second.has_progress_evidence());
    assert_eq!(second_slot.is_ready(), second.is_ready());
    assert!(second_slot.is_selectable());
    for frontier in [
        FrontierKind::Route,
        FrontierKind::Parallel,
        FrontierKind::Reentry,
        FrontierKind::PassiveObserver,
    ] {
        assert_eq!(
            first_slot.is_in_frontier(frontier),
            (first_frontier & frontier.bit()) != 0
        );
        assert_eq!(
            second_slot.is_in_frontier(frontier),
            (second_frontier & frontier.bit()) != 0
        );
    }
}

#[kani::proof]
fn exact_observation_buffer_retains_same_entry_witness_rows() {
    let first = OfferEntryObservedState {
        key: offer_key(7, 1),
        frontier_mask: FrontierKind::Route.bit(),
        flags: OfferEntryObservedState::FLAG_CONTROLLER,
    };
    let second = OfferEntryObservedState {
        key: offer_key(7, 2),
        frontier_mask: FrontierKind::Reentry.bit(),
        flags: OfferEntryObservedState::FLAG_PROGRESS,
    };
    let mut storage = [FrontierObservationSlot::EMPTY; 2];
    let mut observations = super::ObservedEntrySetBuilder::from_slice(&mut storage);
    observations.clear();

    assert_eq!(
        observations.push_exact_observation(first, OfferEntryAdmission::Selectable),
        0
    );
    assert_eq!(
        observations.push_exact_observation(second, OfferEntryAdmission::Selectable),
        1
    );
    let observations = observations.seal();
    assert_eq!(observations.len(), 2);
    assert_eq!(observations.entry_group_end(0), Some(2));
    let first_slot = observations.slot(0).unwrap();
    let second_slot = observations.slot(1).unwrap();
    assert!(first_slot.is_controller());
    assert!(!first_slot.has_progress());
    assert!(!second_slot.is_controller());
    assert!(second_slot.has_progress());
}

fn verify_cursor_target_order_class(left: u8, right: u8) {
    let mut storage = [FrontierObservationSlot::EMPTY; 2];
    let mut observations = super::ObservedEntrySetBuilder::from_slice(&mut storage);
    observations.clear();
    for (entry, scope) in [(left, 1), (right, 2)] {
        observations.push_exact_observation(
            OfferEntryObservedState {
                key: offer_key(entry as u16, scope),
                frontier_mask: FrontierKind::Route.bit(),
                flags: 0,
            },
            OfferEntryAdmission::Selectable,
        );
    }

    let observations = observations.seal();
    let first = observations.entry_idx(0).unwrap();
    let second = observations.entry_idx(1).unwrap();
    assert!(first <= second);
    if left == right {
        assert_eq!(observations.entry_group_end(0), Some(2));
    } else {
        assert_eq!(observations.entry_group_end(0), Some(1));
    }
}

#[kani::proof]
fn exact_observation_buffer_groups_all_cursor_target_order_classes() {
    // Insertion branches only on cursor-target ordering; these calls cover
    // equality and both strict order classes without a redundant u8 product.
    verify_cursor_target_order_class(7, 7);
    verify_cursor_target_order_class(7, 8);
    verify_cursor_target_order_class(8, 7);
}

#[kani::proof]
fn selectable_ready_query_never_admits_an_excluded_exact_witness() {
    let mut storage = [FrontierObservationSlot::EMPTY; 2];
    let mut observations = super::ObservedEntrySetBuilder::from_slice(&mut storage);
    observations.clear();
    observations.push_exact_observation(
        OfferEntryObservedState {
            key: offer_key(7, 1),
            frontier_mask: FrontierKind::Reentry.bit(),
            flags: OfferEntryObservedState::FLAG_READY,
        },
        OfferEntryAdmission::Excluded,
    );
    observations.push_exact_observation(
        OfferEntryObservedState {
            key: offer_key(8, 2),
            frontier_mask: FrontierKind::Reentry.bit(),
            flags: 0,
        },
        OfferEntryAdmission::Selectable,
    );

    assert_eq!(
        observations.seal().first_selectable_ready_entry_except(0),
        None
    );
}

#[kani::proof]
#[kani::should_panic]
fn exact_observation_capacity_exhaustion_is_fail_closed() {
    let mut storage = [FrontierObservationSlot::EMPTY; 1];
    let mut observations = super::ObservedEntrySetBuilder::from_slice(&mut storage);
    observations.clear();
    for entry in [7, 8] {
        observations.push_exact_observation(
            OfferEntryObservedState {
                key: offer_key(entry, 1),
                frontier_mask: FrontierKind::Route.bit(),
                flags: 0,
            },
            OfferEntryAdmission::Selectable,
        );
    }
}

#[kani::proof]
#[kani::should_panic]
fn frontier_observation_rejects_absent_exact_key() {
    let observed = OfferEntryObservedState {
        key: OfferEntryKey::EMPTY,
        frontier_mask: FrontierKind::Route.bit(),
        flags: kani::any(),
    };

    let _ =
        FrontierObservationSlot::from_exact_observation(observed, OfferEntryAdmission::Excluded);
}

#[kani::proof]
#[kani::should_panic]
fn frontier_observation_rejects_bits_outside_the_exact_kind_domain() {
    let candidate = kani::any::<u8>();
    let invalid_bits = !FrontierKind::ALL_BITS;
    let raw_frontier = if candidate & invalid_bits == 0 {
        candidate | (invalid_bits & invalid_bits.wrapping_neg())
    } else {
        candidate
    };
    let observed = OfferEntryObservedState {
        key: offer_key(0, 1),
        frontier_mask: raw_frontier,
        flags: 0,
    };
    let _ =
        FrontierObservationSlot::from_exact_observation(observed, OfferEntryAdmission::Excluded);
}

#[kani::proof]
#[kani::should_panic]
fn frontier_observation_rejects_flags_outside_the_exact_domain() {
    let candidate = kani::any::<u8>();
    let invalid_bits = !OfferEntryObservedState::ALL_FLAGS;
    let raw_flags = if candidate & invalid_bits == 0 {
        candidate | (invalid_bits & invalid_bits.wrapping_neg())
    } else {
        candidate
    };
    let observed = OfferEntryObservedState {
        key: offer_key(0, 1),
        frontier_mask: FrontierKind::Route.bit(),
        flags: raw_flags,
    };
    let _ =
        FrontierObservationSlot::from_exact_observation(observed, OfferEntryAdmission::Excluded);
}
