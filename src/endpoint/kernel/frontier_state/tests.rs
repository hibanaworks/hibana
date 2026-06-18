use super::*;
use core::mem::MaybeUninit;

use crate::endpoint::kernel::frontier::ObservedEntrySummary;
use crate::global::const_dsl::ScopeId;
use crate::global::role_program::lane_word_count;

fn test_scope(raw: u16) -> ScopeId {
    ScopeId::parallel(raw)
}

fn observed_key_storage(
    entries: &[usize],
) -> (
    std::vec::Vec<FrontierObservationSlot>,
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
    let mut offer_lanes = std::vec![0usize; lane_word_count(1)];
    let mut key = FrontierObservationKey::from_parts(
        key_slots.as_mut_ptr(),
        key_slots.len(),
        offer_lanes.as_mut_ptr(),
        lane_word_count(1),
    );
    key.clear();
    key.set_active_entries_from(active);
    (key_slots, offer_lanes, key)
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
            RootFrontierStorage {
                rows: rows.as_mut_ptr(),
                active_entries: active.as_mut_ptr(),
                observed_key_slots: observed.as_mut_ptr(),
                observed_key_offer_lanes: core::ptr::null_mut(),
            },
            RootFrontierCapacity {
                row_count: ROWS,
                pool_capacity: 1,
                observed_key_lane_word_count: 0,
            },
        );
    }
    let mut table = unsafe { table.assume_init() };

    for idx in 0..ROWS {
        table.prepare_row(idx, test_scope(idx as u16 + 1));
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
fn root_frontier_shared_active_pool_stays_packed_after_row_removal() {
    let mut rows = [RootFrontierState::EMPTY; 4];
    let mut active = [ActiveEntrySlot::EMPTY; 8];
    let mut observed = [FrontierObservationSlot::EMPTY; 8];
    let mut observed_offer_lanes = [0usize; 3 * lane_word_count(1)];
    let mut table = MaybeUninit::<RootFrontierTable>::uninit();
    unsafe {
        RootFrontierTable::init_from_parts(
            table.as_mut_ptr(),
            RootFrontierStorage {
                rows: rows.as_mut_ptr(),
                active_entries: active.as_mut_ptr(),
                observed_key_slots: observed.as_mut_ptr(),
                observed_key_offer_lanes: observed_offer_lanes.as_mut_ptr(),
            },
            RootFrontierCapacity {
                row_count: 3,
                pool_capacity: 4,
                observed_key_lane_word_count: lane_word_count(1),
            },
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
    let mut table = MaybeUninit::<RootFrontierTable>::uninit();
    unsafe {
        RootFrontierTable::init_from_parts(
            table.as_mut_ptr(),
            RootFrontierStorage {
                rows: rows.as_mut_ptr(),
                active_entries: active.as_mut_ptr(),
                observed_key_slots: observed.as_mut_ptr(),
                observed_key_offer_lanes: observed_offer_lanes.as_mut_ptr(),
            },
            RootFrontierCapacity {
                row_count: 3,
                pool_capacity: 4,
                observed_key_lane_word_count: lane_word_count(1),
            },
        );
    }
    let mut table = unsafe { table.assume_init() };

    table.prepare_row(0, test_scope(1));
    table.prepare_row(1, test_scope(2));
    assert!(table.insert_root_active_entry(0, 10, 0));
    assert!(table.insert_root_active_entry(0, 11, 1));
    assert!(table.insert_root_active_entry(1, 20, 0));
    let (_key0_slots, _key0_offer_lanes, key0) = observed_key_storage(&[10, 11]);
    let (_key1_slots, _key1_offer_lanes, key1) = observed_key_storage(&[20]);
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
    let mut table = MaybeUninit::<RootFrontierTable>::uninit();
    unsafe {
        RootFrontierTable::init_from_parts(
            table.as_mut_ptr(),
            RootFrontierStorage {
                rows: rows.as_mut_ptr(),
                active_entries: active.as_mut_ptr(),
                observed_key_slots: observed.as_mut_ptr(),
                observed_key_offer_lanes: observed_offer_lanes.as_mut_ptr(),
            },
            RootFrontierCapacity {
                row_count: 3,
                pool_capacity: 4,
                observed_key_lane_word_count: lane_word_count(1),
            },
        );
    }
    let mut table = unsafe { table.assume_init() };

    table.prepare_row(0, test_scope(1));
    assert!(table.insert_root_active_entry(0, 10, 0));
    let (_key_slots, _key_offer_lanes, key) = observed_key_storage(&[10]);
    table.replace_root_observed_key(0, key);
    assert_eq!(table.observed_key(0).len(), 1);

    assert!(table.insert_root_active_entry(0, 11, 1));
    assert_eq!(table.observed_key(0).len(), 0);
    assert!(table[0].observed_entries == ObservedEntrySummary::EMPTY);
}
