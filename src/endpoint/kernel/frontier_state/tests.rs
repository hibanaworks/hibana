use super::*;
use core::mem::MaybeUninit;

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
    assert_eq!(table.active_entry_set(0).entry_at(0), Some(20));
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
    assert_eq!(entries.entry_at(0), Some(10));
    assert_eq!(entries.entry_at(1), Some(20));
    assert_eq!(entries.entry_at(2), Some(30));

    assert!(table.remove_root_active_entry(0, 20));

    let entries = table.active_entry_set(0);
    assert_eq!(entries.len(), 2);
    assert_eq!(entries.entry_at(0), Some(10));
    assert_eq!(entries.entry_at(1), Some(30));
    assert_eq!(table.active_pool_used(), 2);
}
