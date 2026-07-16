use super::{
    ActiveEntrySetBuilder, ActiveEntrySlot, EntryBuffer, FrontierObservationSlot, StateIndex,
};
use crate::endpoint::kernel::frontier::{
    FrontierKind, OfferEntryAdmission, OfferEntryObservedState,
};
use crate::global::{
    const_dsl::{ScopeId, ScopeKind},
    role_program::RuntimeRoleFootprint,
};

#[kani::proof]
fn frontier_entry_capacity_preserves_the_full_lane_domain() {
    let mut storage = [ActiveEntrySlot::EMPTY; 256];
    let active_lane_count: u16 = kani::any();
    kani::assume(active_lane_count as usize <= storage.len());
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
    /* SAFETY: the symbolic capacity is bounded by the live storage array and
    the builder owns it exclusively for this proof. */
    let entries = unsafe { ActiveEntrySetBuilder::from_parts(storage.as_mut_ptr(), capacity) };

    assert_eq!(entries.capacity(), capacity);
    assert_eq!(entries.capacity(), active_lane_count as usize);
    assert_eq!(entries.len(), 0);
    let entries = entries.seal();
    assert_eq!(entries.len(), 0);
    assert_eq!(capacity, active_lane_count as usize);
}

#[kani::proof]
#[kani::should_panic]
fn nonempty_frontier_entry_buffer_rejects_null_storage() {
    /* SAFETY: this deliberately violates the constructor contract to prove the
    owner fails before dereferencing null storage. */
    let _ = unsafe { EntryBuffer::<ActiveEntrySlot>::from_parts(core::ptr::null_mut(), 1) };
}

#[kani::proof]
fn frontier_observation_packing_is_exact() {
    let raw_flags = kani::any::<u8>();
    let raw_frontier = kani::any::<u8>();
    kani::assume(raw_frontier & !FrontierKind::ALL_BITS == 0);
    let selectable = kani::any::<bool>();
    let admission = if selectable {
        OfferEntryAdmission::Selectable
    } else {
        OfferEntryAdmission::Excluded
    };
    let observed = OfferEntryObservedState {
        scope_id: ScopeId::new(ScopeKind::Route, 1),
        frontier_mask: raw_frontier,
        flags: raw_flags,
    };
    let mut slot = FrontierObservationSlot::new(StateIndex::new(0));
    slot.record(observed, raw_frontier, admission);

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
#[kani::should_panic]
fn frontier_observation_rejects_bits_outside_the_exact_kind_domain() {
    let raw_frontier = kani::any::<u8>();
    kani::assume(raw_frontier & !FrontierKind::ALL_BITS != 0);
    let observed = OfferEntryObservedState {
        scope_id: ScopeId::new(ScopeKind::Route, 1),
        frontier_mask: raw_frontier,
        flags: 0,
    };
    let mut slot = FrontierObservationSlot::new(StateIndex::new(0));

    slot.record(observed, raw_frontier, OfferEntryAdmission::Excluded);
}
