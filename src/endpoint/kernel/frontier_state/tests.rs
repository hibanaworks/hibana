use super::*;
use core::mem::MaybeUninit;
use std::panic::{AssertUnwindSafe, catch_unwind};

use crate::global::const_dsl::ScopeId;

fn test_scope(raw: u16) -> ScopeId {
    ScopeId::parallel(raw)
}

fn init_table<'a>(
    rows: &'a mut [RootFrontierState],
    active: &'a mut [ActiveEntrySlot],
    row_count: usize,
) -> RootFrontierTable {
    let mut table = MaybeUninit::<RootFrontierTable>::uninit();
    unsafe {
        RootFrontierTable::init_from_parts(
            table.as_mut_ptr(),
            RootFrontierStorage {
                rows: rows.as_mut_ptr(),
                active_entries: active.as_mut_ptr(),
            },
            RootFrontierCapacity {
                row_count,
                pool_capacity: active.len(),
            },
        );
        table.assume_init()
    }
}

fn active_slot(table: &RootFrontierTable, idx: usize) -> ActiveEntrySlot {
    assert!(idx < table.pool_capacity());
    /* SAFETY: `idx` was bounded by this table's initialized active-entry pool. */
    unsafe { *table.active_entries.add(idx) }
}

#[test]
fn root_frontier_table_accepts_full_u8_lane_domain_rows() {
    const ROWS: usize = 256;
    let mut rows = std::vec::Vec::with_capacity(ROWS);
    rows.resize(ROWS, RootFrontierState::EMPTY);
    let mut active = [ActiveEntrySlot::EMPTY; 1];
    let mut table = init_table(&mut rows, &mut active, ROWS);

    for idx in 0..ROWS {
        table.prepare_row(idx, test_scope(idx as u16 + 1));
    }

    assert_eq!(table.capacity(), ROWS);
    assert_eq!(table.len(), ROWS);
    assert_eq!(table[255].root, test_scope(256));
}

#[test]
fn root_frontier_shared_active_pool_stays_packed_after_row_removal() {
    let mut rows = [RootFrontierState::EMPTY; 4];
    let mut active = [ActiveEntrySlot::EMPTY; 8];
    let mut table = init_table(&mut rows, &mut active, 3);

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
    assert_eq!(
        table
            .active_entry_set(0)
            .slot_at(0)
            .map(|slot| slot.entry.as_usize()),
        Some(20)
    );
}

#[test]
fn root_frontier_active_pool_stays_ordered_after_insert_and_remove() {
    let mut rows = [RootFrontierState::EMPTY; 2];
    let mut active = [ActiveEntrySlot::EMPTY; 4];
    let mut table = init_table(&mut rows, &mut active, 2);

    table.prepare_row(0, test_scope(1));
    assert!(table.insert_root_active_entry(0, 30, 2));
    assert!(table.insert_root_active_entry(0, 10, 0));
    assert!(table.insert_root_active_entry(0, 20, 1));

    let entries = table.active_entry_set(0);
    assert_eq!(
        entries.slot_at(0).map(|slot| slot.entry.as_usize()),
        Some(10)
    );
    assert_eq!(
        entries.slot_at(1).map(|slot| slot.entry.as_usize()),
        Some(20)
    );
    assert_eq!(
        entries.slot_at(2).map(|slot| slot.entry.as_usize()),
        Some(30)
    );

    assert!(table.remove_root_active_entry(0, 20));

    let entries = table.active_entry_set(0);
    assert_eq!(entries.len(), 2);
    assert_eq!(
        entries.slot_at(0).map(|slot| slot.entry.as_usize()),
        Some(10)
    );
    assert_eq!(
        entries.slot_at(1).map(|slot| slot.entry.as_usize()),
        Some(30)
    );
    assert_eq!(table.active_pool_used(), 2);
}

#[test]
fn root_frontier_insert_validates_all_offsets_before_pool_mutation() {
    let mut rows = [RootFrontierState::EMPTY; 3];
    let mut active = [ActiveEntrySlot::EMPTY; 2];
    let mut table = init_table(&mut rows, &mut active, 3);
    for idx in 0..3 {
        table.prepare_row(idx, test_scope(idx as u16 + 1));
    }
    table[1].active_start = u16::MAX;
    table[2].active_start = 0;
    let active_before = active_slot(&table, 0);

    let result = catch_unwind(AssertUnwindSafe(|| {
        let _ = table.insert_root_active_entry(0, 10, 0);
    }));

    assert!(result.is_err());
    assert_eq!(table[0].active_len, 0);
    assert_eq!(table[1].active_start, u16::MAX);
    assert!(active_slot(&table, 0) == active_before);
}

#[test]
fn root_frontier_remove_validates_all_offsets_before_pool_mutation() {
    let mut rows = [RootFrontierState::EMPTY; 3];
    let mut active = [ActiveEntrySlot::EMPTY; 2];
    let mut table = init_table(&mut rows, &mut active, 3);
    for idx in 0..3 {
        table.prepare_row(idx, test_scope(idx as u16 + 1));
    }
    assert!(table.insert_root_active_entry(0, 10, 0));
    table[1].active_start = 0;
    table[2].active_start = 1;
    let active_before = active_slot(&table, 0);

    let result = catch_unwind(AssertUnwindSafe(|| {
        let _ = table.remove_root_active_entry(0, 10);
    }));

    assert!(result.is_err());
    assert_eq!(table[0].active_len, 1);
    assert_eq!(table[1].active_start, 0);
    assert!(active_slot(&table, 0) == active_before);
}

#[test]
fn root_frontier_row_remove_validates_offsets_before_compaction() {
    let mut rows = [RootFrontierState::EMPTY; 2];
    let mut active = [ActiveEntrySlot::EMPTY; 3];
    let mut table = init_table(&mut rows, &mut active, 2);
    table.prepare_row(0, test_scope(1));
    table.prepare_row(1, test_scope(2));
    assert!(table.insert_root_active_entry(0, 10, 0));
    assert!(table.insert_root_active_entry(0, 20, 1));
    table[1].active_start = 1;
    let first_before = active_slot(&table, 0);
    let second_before = active_slot(&table, 1);

    let result = catch_unwind(AssertUnwindSafe(|| table.remove_root_row(0)));

    assert!(result.is_err());
    assert_eq!(table[0].root, test_scope(1));
    assert_eq!(table[0].active_len, 2);
    assert_eq!(table[1].root, test_scope(2));
    assert_eq!(table[1].active_start, 1);
    assert!(active_slot(&table, 0) == first_before);
    assert!(active_slot(&table, 1) == second_before);
}

#[test]
#[should_panic]
fn missing_root_frontier_cannot_silently_detach_a_lane() {
    let mut rows = [RootFrontierState::EMPTY; 1];
    let mut active = [ActiveEntrySlot::EMPTY; 1];
    let table = init_table(&mut rows, &mut active, 1);
    let mut state = FrontierState {
        root_frontier_state: table,
        visited_entries: core::ptr::null_mut(),
    };
    let mut info = LaneOfferState::EMPTY;
    info.parallel_root = test_scope(1);

    state.detach_lane_from_root_frontier(info);
}
