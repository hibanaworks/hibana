use super::{
    ActiveEntrySlot, RootFrontierCapacity, RootFrontierState, RootFrontierStorage,
    RootFrontierTable, ScopeId,
};
use core::mem::MaybeUninit;

fn init_table<const ROWS: usize, const ENTRIES: usize>(
    rows: &mut [RootFrontierState; ROWS],
    active_entries: &mut [ActiveEntrySlot; ENTRIES],
) -> RootFrontierTable {
    let mut table = MaybeUninit::<RootFrontierTable>::uninit();
    /* SAFETY: both arrays are fully allocated for the exact capacities passed
    here, and `table` remains unpublished until initialization completes. */
    unsafe {
        RootFrontierTable::init_from_parts(
            table.as_mut_ptr(),
            RootFrontierStorage {
                rows: rows.as_mut_ptr(),
                active_entries: active_entries.as_mut_ptr(),
            },
            RootFrontierCapacity {
                row_count: rows.len(),
                pool_capacity: active_entries.len(),
            },
        );
        table.assume_init()
    }
}

#[kani::proof]
fn root_frontier_owner_slots_preserve_symbolic_lane_order() {
    let mut rows = [RootFrontierState::EMPTY; 1];
    let mut storage = [ActiveEntrySlot::EMPTY; 2];
    let mut table = init_table(&mut rows, &mut storage);
    let reverse_order: bool = kani::any();
    let lane_a = if reverse_order { 9 } else { 2 };
    let lane_b = if reverse_order { 2 } else { 9 };

    table.prepare_row(0, ScopeId::parallel(1));
    assert!(table.insert_root_active_entry(0, 10, lane_a));
    assert!(table.insert_root_active_entry(0, 20, lane_b));
    let first_is_a = lane_a < lane_b;
    let entries = table.active_entry_set(0);
    let first = crate::invariant_some(entries.slot_at(0));
    let second = crate::invariant_some(entries.slot_at(1));
    assert_eq!(first.entry.as_usize(), if first_is_a { 10 } else { 20 });
    assert_eq!(first.lane_idx, if first_is_a { lane_a } else { lane_b });
    assert_eq!(second.entry.as_usize(), if first_is_a { 20 } else { 10 });
    assert_eq!(second.lane_idx, if first_is_a { lane_b } else { lane_a });
}

fn verify_root_frontier_owner_slot_after_removal<const REMOVE_FIRST: bool>() {
    let mut rows = [RootFrontierState::EMPTY; 1];
    let mut storage = [ActiveEntrySlot::EMPTY; 2];
    let mut table = init_table(&mut rows, &mut storage);
    table.prepare_row(0, ScopeId::parallel(1));
    assert!(table.insert_root_active_entry(0, 10, 2));
    assert!(table.insert_root_active_entry(0, 20, 9));
    assert!(table.remove_root_active_entry(0, if REMOVE_FIRST { 10 } else { 20 }));
    let remaining = crate::invariant_some(table.active_entry_set(0).slot_at(0));
    assert_eq!(
        remaining.entry.as_usize(),
        if REMOVE_FIRST { 20 } else { 10 }
    );
    assert_eq!(remaining.lane_idx, if REMOVE_FIRST { 9 } else { 2 });
}

#[kani::proof]
fn root_frontier_owner_slot_survives_first_entry_removal() {
    verify_root_frontier_owner_slot_after_removal::<true>();
}

#[kani::proof]
fn root_frontier_owner_slot_survives_last_entry_removal() {
    verify_root_frontier_owner_slot_after_removal::<false>();
}

#[kani::proof]
fn root_frontier_owner_slot_survives_row_compaction() {
    let mut rows = [RootFrontierState::EMPTY; 2];
    let mut storage = [ActiveEntrySlot::EMPTY; 2];
    let mut table = init_table(&mut rows, &mut storage);
    table.prepare_row(0, ScopeId::parallel(1));
    assert!(table.insert_root_active_entry(0, 10, 2));
    table.prepare_row(1, ScopeId::parallel(2));
    assert!(table.insert_root_active_entry(1, 30, u8::MAX));

    table.remove_root_row(0);
    assert_eq!(table[0].root, ScopeId::parallel(2));
    let compacted = crate::invariant_some(table.active_entry_set(0).slot_at(0));
    assert_eq!(compacted.entry.as_usize(), 30);
    assert_eq!(compacted.lane_idx, u8::MAX);
}
