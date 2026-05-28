use super::*;
use core::mem::MaybeUninit;

use crate::endpoint::kernel::offer::FrontierObservationDomain;
use crate::global::const_dsl::ScopeId;
use crate::global::role_program::lane_word_count;

fn test_scope(raw: u64) -> ScopeId {
    ScopeId::from_raw(raw)
}

fn observed_key_storage(
    entries: &[usize],
) -> (
    std::vec::Vec<FrontierObservationSlot>,
    std::vec::Vec<usize>,
    std::vec::Vec<usize>,
    FrontierObservationKey,
) {
    let mut active_slots = std::vec::Vec::with_capacity(entries.len().max(1));
    active_slots.resize(entries.len().max(1), ActiveEntrySlot::EMPTY);
    let mut active = ActiveEntrySet::from_parts(active_slots.as_mut_ptr(), active_slots.len());
    active.clear();
    let mut idx = 0usize;
    while idx < entries.len() {
        assert!(active.insert_entry(entries[idx], idx as u8));
        idx += 1;
    }
    let mut key_slots = std::vec::Vec::with_capacity(entries.len().max(1));
    key_slots.resize(entries.len().max(1), FrontierObservationSlot::EMPTY);
    let mut offer_lanes = std::vec::Vec::with_capacity(lane_word_count(1));
    offer_lanes.resize(lane_word_count(1), 0usize);
    let mut binding_nonempty_lanes = std::vec::Vec::with_capacity(lane_word_count(1));
    binding_nonempty_lanes.resize(lane_word_count(1), 0usize);
    let mut key = FrontierObservationKey::from_parts(
        key_slots.as_mut_ptr(),
        key_slots.len(),
        offer_lanes.as_mut_ptr(),
        binding_nonempty_lanes.as_mut_ptr(),
        lane_word_count(1),
    );
    key.clear();
    key.set_active_entries_from(active);
    (key_slots, offer_lanes, binding_nonempty_lanes, key)
}

#[test]
fn root_frontier_table_accepts_full_u8_lane_domain_rows() {
    const ROWS: usize = 256;
    let mut rows = std::vec::Vec::with_capacity(ROWS);
    rows.resize(ROWS, RootFrontierState::EMPTY);
    let mut active = [ActiveEntrySlot::EMPTY; 1];
    let mut observed = [FrontierObservationSlot::EMPTY; 1];
    let mut table = MaybeUninit::<RootFrontierTable>::uninit();
    unsafe {
        RootFrontierTable::init_from_parts(
            table.as_mut_ptr(),
            rows.as_mut_ptr(),
            active.as_mut_ptr(),
            observed.as_mut_ptr(),
            core::ptr::null_mut(),
            core::ptr::null_mut(),
            ROWS,
            1,
            0,
        );
    }
    let mut table = unsafe { table.assume_init() };

    for idx in 0..ROWS {
        table.prepare_row(idx, test_scope(idx as u64 + 1));
    }

    assert_eq!(table.capacity(), ROWS);
    assert_eq!(table.len(), ROWS);
    assert_eq!(table[255].root, test_scope(256));
}

#[test]
fn global_active_entry_set_stays_packed_after_removal() {
    let mut slots = [ActiveEntrySlot::EMPTY; 4];
    let mut active = ActiveEntrySet::EMPTY;
    unsafe {
        ActiveEntrySet::init_from_parts(
            (&mut active) as *mut ActiveEntrySet,
            slots.as_mut_ptr(),
            4,
        );
    }
    assert!(active.insert_entry(10, 0));
    assert!(active.insert_entry(20, 1));
    assert!(active.remove_entry(10));
    assert_eq!(active.len(), 1);
    assert_eq!(active.entry_at(0), Some(20));
    assert_eq!(active.occupancy_mask(), 0b0000_0001);
}

#[test]
fn global_frontier_cache_uses_external_key_storage() {
    let mut global_observed = [FrontierObservationSlot::EMPTY; 4];
    let mut global_observed_offer_lanes = [0usize; lane_word_count(1)];
    let mut global_observed_binding_nonempty_lanes = [0usize; lane_word_count(1)];
    let mut root_rows = [RootFrontierState::EMPTY; 2];
    let mut root_active = [ActiveEntrySlot::EMPTY; 4];
    let mut root_observed = [FrontierObservationSlot::EMPTY; 4];
    let mut root_observed_offer_lanes = [0usize; 2 * lane_word_count(1)];
    let mut root_observed_binding_nonempty_lanes = [0usize; 2 * lane_word_count(1)];
    let mut offer_slots = [OfferEntrySlot::EMPTY; 4];
    let mut observed_slots = [FrontierObservationSlot::EMPTY; 4];
    let mut frontier_state = MaybeUninit::<FrontierState>::uninit();
    unsafe {
        FrontierState::init_empty(
            frontier_state.as_mut_ptr(),
            root_rows.as_mut_ptr(),
            root_active.as_mut_ptr(),
            root_observed.as_mut_ptr(),
            root_observed_offer_lanes.as_mut_ptr(),
            root_observed_binding_nonempty_lanes.as_mut_ptr(),
            offer_slots.as_mut_ptr(),
            2,
            4,
            lane_word_count(1),
            4,
        );
    }
    let mut frontier_state = unsafe { frontier_state.assume_init() };
    let (_cached_key_slots, _cached_offer_lanes, _cached_binding_nonempty_lanes, cached_key) =
        observed_key_storage(&[7, 9]);
    let mut stored_key = FrontierObservationKey::from_parts(
        global_observed.as_mut_ptr(),
        4,
        global_observed_offer_lanes.as_mut_ptr(),
        global_observed_binding_nonempty_lanes.as_mut_ptr(),
        lane_word_count(1),
    );
    stored_key.clear();
    let mut observed_entries = ObservedEntrySet::from_parts(observed_slots.as_mut_ptr(), 4);
    assert_eq!(observed_entries.insert_entry(7), Some((0b0000_0001, true)));
    assert_eq!(observed_entries.insert_entry(9), Some((0b0000_0010, true)));

    frontier_state.store_frontier_observation(
        FrontierObservationDomain::global(),
        stored_key,
        cached_key,
        observed_entries,
    );

    let (cached_key_after_store, cached_entries_after_store) =
        frontier_state.frontier_observation_cache(FrontierObservationDomain::global(), stored_key);
    assert!(cached_key_after_store == cached_key);
    assert_eq!(cached_entries_after_store.entry_bit(7), 0b0000_0001);
    assert_eq!(cached_entries_after_store.entry_bit(9), 0b0000_0010);
    let cached_again = frontier_state.cached_frontier_observed_entries(
        FrontierObservationDomain::global(),
        stored_key,
        &cached_key,
    );
    assert!(cached_again.is_some());
    let cached_again = cached_again.unwrap_or(ObservedEntrySet::EMPTY);
    assert_eq!(cached_again.entry_bit(7), 0b0000_0001);
    assert_eq!(cached_again.entry_bit(9), 0b0000_0010);
}

#[test]
fn root_frontier_shared_active_pool_stays_packed_after_row_removal() {
    let mut rows = [RootFrontierState::EMPTY; 4];
    let mut active = [ActiveEntrySlot::EMPTY; 8];
    let mut observed = [FrontierObservationSlot::EMPTY; 8];
    let mut observed_offer_lanes = [0usize; 3 * lane_word_count(1)];
    let mut observed_binding_nonempty_lanes = [0usize; 3 * lane_word_count(1)];
    let mut table = MaybeUninit::<RootFrontierTable>::uninit();
    unsafe {
        RootFrontierTable::init_from_parts(
            table.as_mut_ptr(),
            rows.as_mut_ptr(),
            active.as_mut_ptr(),
            observed.as_mut_ptr(),
            observed_offer_lanes.as_mut_ptr(),
            observed_binding_nonempty_lanes.as_mut_ptr(),
            3,
            4,
            lane_word_count(1),
        );
    }
    let mut table = unsafe { table.assume_init() };

    table.prepare_row(0, test_scope(1));
    table.prepare_row(1, test_scope(2));
    assert!(table.insert_root_active_entry(0, 10, 0));
    assert!(table.insert_root_active_entry(1, 20, 1));

    assert_eq!(table.active_pool_used(), 2);
    assert_eq!(table[0].active_start, 0);
    assert_eq!(table[1].active_start, 1);

    table.remove_root_row(0);

    assert_eq!(table.len(), 1);
    assert_eq!(table[0].root, test_scope(2));
    assert_eq!(table[0].active_start, 0);
    assert_eq!(table[0].active_len, 1);
    assert_eq!(table.active_pool_used(), 1);
    assert_eq!(table.active_entry_set(0).entry_at(0), Some(20));
}

#[test]
fn root_frontier_shared_observed_pool_stays_packed_after_row_removal() {
    let mut rows = [RootFrontierState::EMPTY; 4];
    let mut active = [ActiveEntrySlot::EMPTY; 8];
    let mut observed = [FrontierObservationSlot::EMPTY; 8];
    let mut observed_offer_lanes = [0usize; 3 * lane_word_count(1)];
    let mut observed_binding_nonempty_lanes = [0usize; 3 * lane_word_count(1)];
    let mut table = MaybeUninit::<RootFrontierTable>::uninit();
    unsafe {
        RootFrontierTable::init_from_parts(
            table.as_mut_ptr(),
            rows.as_mut_ptr(),
            active.as_mut_ptr(),
            observed.as_mut_ptr(),
            observed_offer_lanes.as_mut_ptr(),
            observed_binding_nonempty_lanes.as_mut_ptr(),
            3,
            4,
            lane_word_count(1),
        );
    }
    let mut table = unsafe { table.assume_init() };

    table.prepare_row(0, test_scope(1));
    table.prepare_row(1, test_scope(2));
    assert!(table.insert_root_active_entry(0, 10, 0));
    assert!(table.insert_root_active_entry(0, 11, 1));
    assert!(table.insert_root_active_entry(1, 20, 0));
    let (_key0_slots, _key0_offer_lanes, _key0_binding_nonempty_lanes, key0) =
        observed_key_storage(&[10, 11]);
    let (_key1_slots, _key1_offer_lanes, _key1_binding_nonempty_lanes, key1) =
        observed_key_storage(&[20]);
    table.replace_root_observed_key(0, key0);
    table.replace_root_observed_key(1, key1);

    assert_eq!(table.active_pool_used(), 3);
    assert_eq!(table[0].active_start, 0);
    assert_eq!(table[1].active_start, 2);

    table.remove_root_row(0);

    assert_eq!(table.len(), 1);
    assert_eq!(table[0].root, test_scope(2));
    assert_eq!(table[0].active_start, 0);
    assert_eq!(table[0].active_len, 1);
    let key = table.observed_key(0);
    assert_eq!(key.len(), 1);
    assert!(key.contains_entry(20));
    assert_eq!(table.active_pool_used(), 1);
}

#[test]
fn root_frontier_observed_cache_invalidates_on_active_entry_change() {
    let mut rows = [RootFrontierState::EMPTY; 4];
    let mut active = [ActiveEntrySlot::EMPTY; 8];
    let mut observed = [FrontierObservationSlot::EMPTY; 8];
    let mut observed_offer_lanes = [0usize; 3 * lane_word_count(1)];
    let mut observed_binding_nonempty_lanes = [0usize; 3 * lane_word_count(1)];
    let mut table = MaybeUninit::<RootFrontierTable>::uninit();
    unsafe {
        RootFrontierTable::init_from_parts(
            table.as_mut_ptr(),
            rows.as_mut_ptr(),
            active.as_mut_ptr(),
            observed.as_mut_ptr(),
            observed_offer_lanes.as_mut_ptr(),
            observed_binding_nonempty_lanes.as_mut_ptr(),
            3,
            4,
            lane_word_count(1),
        );
    }
    let mut table = unsafe { table.assume_init() };

    table.prepare_row(0, test_scope(1));
    assert!(table.insert_root_active_entry(0, 10, 0));
    let (_key_slots, _key_offer_lanes, _key_binding_nonempty_lanes, key) =
        observed_key_storage(&[10]);
    table.replace_root_observed_key(0, key);
    assert_eq!(table.observed_key(0).len(), 1);

    assert!(table.insert_root_active_entry(0, 11, 1));
    assert_eq!(table.observed_key(0).len(), 0);
    assert!(table[0].observed_entries == ObservedEntrySummary::EMPTY);
}

#[test]
fn next_observation_epoch_wrap_clears_shared_root_observed_pool() {
    let mut global_observed = [FrontierObservationSlot::EMPTY; 4];
    let mut global_observed_offer_lanes = [0usize; lane_word_count(1)];
    let mut global_observed_binding_nonempty_lanes = [0usize; lane_word_count(1)];
    let mut root_rows = [RootFrontierState::EMPTY; 2];
    let mut root_active = [ActiveEntrySlot::EMPTY; 4];
    let mut root_observed = [FrontierObservationSlot::EMPTY; 4];
    let mut root_observed_offer_lanes = [0usize; 2 * lane_word_count(1)];
    let mut root_observed_binding_nonempty_lanes = [0usize; 2 * lane_word_count(1)];
    let mut offer_slots = [OfferEntrySlot::EMPTY; 4];
    let mut frontier_state = MaybeUninit::<FrontierState>::uninit();
    unsafe {
        FrontierState::init_empty(
            frontier_state.as_mut_ptr(),
            root_rows.as_mut_ptr(),
            root_active.as_mut_ptr(),
            root_observed.as_mut_ptr(),
            root_observed_offer_lanes.as_mut_ptr(),
            root_observed_binding_nonempty_lanes.as_mut_ptr(),
            offer_slots.as_mut_ptr(),
            2,
            4,
            lane_word_count(1),
            4,
        );
    }
    let mut frontier_state = unsafe { frontier_state.assume_init() };
    let mut global_frontier_observed_key = FrontierObservationKey::from_parts(
        global_observed.as_mut_ptr(),
        4,
        global_observed_offer_lanes.as_mut_ptr(),
        global_observed_binding_nonempty_lanes.as_mut_ptr(),
        lane_word_count(1),
    );
    global_frontier_observed_key.clear();
    frontier_state
        .root_frontier_state
        .prepare_row(0, test_scope(1));
    assert!(
        frontier_state
            .root_frontier_state
            .insert_root_active_entry(0, 3, 0)
    );
    assert!(
        frontier_state
            .root_frontier_state
            .insert_root_active_entry(0, 5, 1)
    );
    let (_root_key_slots, _root_key_offer_lanes, _root_key_binding_nonempty_lanes, root_key) =
        observed_key_storage(&[3, 5]);
    frontier_state
        .root_frontier_state
        .replace_root_observed_key(0, root_key);
    frontier_state.root_frontier_state[0].observed_entries = ObservedEntrySummary {
        controller_mask: 1,
        dynamic_controller_mask: 1,
        progress_mask: 1,
        ready_arm_mask: 1,
        ready_mask: 1,
    };
    let (
        _global_key_slots,
        _global_key_offer_lanes,
        _global_key_binding_nonempty_lanes,
        global_key,
    ) = observed_key_storage(&[7]);
    global_frontier_observed_key.copy_from(global_key);
    frontier_state.global_frontier_observed = ObservedEntrySummary {
        controller_mask: 1,
        dynamic_controller_mask: 0,
        progress_mask: 1,
        ready_arm_mask: 0,
        ready_mask: 1,
    };
    frontier_state.frontier_observation_epoch = u16::MAX;

    assert_eq!(
        frontier_state.next_observation_epoch(&mut global_frontier_observed_key),
        1
    );
    assert_eq!(global_frontier_observed_key.len(), 0);
    assert!(frontier_state.global_frontier_observed == ObservedEntrySummary::EMPTY);
    assert!(frontier_state.root_frontier_state[0].observed_entries == ObservedEntrySummary::EMPTY);
    assert_eq!(frontier_state.root_frontier_state.observed_key(0).len(), 0);
}
