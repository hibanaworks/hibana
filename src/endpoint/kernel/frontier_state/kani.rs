use super::{
    ActiveEntrySlot, OfferEntryKey, RootFrontierCapacity, RootFrontierState, RootFrontierStorage,
    RootFrontierTable, ScopeId,
};
use crate::global::typestate::StateIndex;
use core::mem::MaybeUninit;

fn offer_key(entry: u16, scope: u16) -> OfferEntryKey {
    OfferEntryKey::new(ScopeId::route(scope), StateIndex::new(entry)).unwrap()
}

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
    table.insert_root_active_entry(0, offer_key(10, 1), lane_a);
    table.insert_root_active_entry(0, offer_key(20, 2), lane_b);
    let first_is_a = lane_a < lane_b;
    let entries = table.active_entry_set(0);
    let first = crate::invariant_some(entries.slot_at(0));
    let second = crate::invariant_some(entries.slot_at(1));
    assert_eq!(
        first.key.entry().as_usize(),
        if first_is_a { 10 } else { 20 }
    );
    assert_eq!(first.lane_idx, if first_is_a { lane_a } else { lane_b });
    assert_eq!(
        second.key.entry().as_usize(),
        if first_is_a { 20 } else { 10 }
    );
    assert_eq!(second.lane_idx, if first_is_a { lane_b } else { lane_a });
}

#[kani::proof]
fn root_frontier_owner_slots_distinguish_scope_at_same_entry() {
    let mut rows = [RootFrontierState::EMPTY; 1];
    let mut storage = [ActiveEntrySlot::EMPTY; 2];
    let mut table = init_table(&mut rows, &mut storage);
    let first = offer_key(10, 1);
    let second = offer_key(10, 2);

    table.prepare_row(0, ScopeId::parallel(1));
    table.insert_root_active_entry(0, first, 2);
    table.insert_root_active_entry(0, second, 9);

    let entries = table.active_entry_set(0);
    assert_eq!(entries.len(), 2);
    assert_eq!(entries.slot_at(0).unwrap().key, first);
    assert_eq!(entries.slot_at(1).unwrap().key, second);
}

fn verify_root_frontier_owner_slot_after_removal<const REMOVE_FIRST: bool>() {
    let mut rows = [RootFrontierState::EMPTY; 1];
    let mut storage = [ActiveEntrySlot::EMPTY; 2];
    let mut table = init_table(&mut rows, &mut storage);
    table.prepare_row(0, ScopeId::parallel(1));
    table.insert_root_active_entry(0, offer_key(10, 1), 2);
    table.insert_root_active_entry(0, offer_key(20, 2), 9);
    table.remove_root_active_entry(
        0,
        if REMOVE_FIRST {
            offer_key(10, 1)
        } else {
            offer_key(20, 2)
        },
    );
    let remaining = crate::invariant_some(table.active_entry_set(0).slot_at(0));
    assert_eq!(
        remaining.key.entry().as_usize(),
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
    table.insert_root_active_entry(0, offer_key(10, 1), 2);
    table.prepare_row(1, ScopeId::parallel(2));
    table.insert_root_active_entry(1, offer_key(30, 3), u8::MAX);

    table.remove_root_row(0);
    assert_eq!(table[0].root, ScopeId::parallel(2));
    let compacted = crate::invariant_some(table.active_entry_set(0).slot_at(0));
    assert_eq!(compacted.key.entry().as_usize(), 30);
    assert_eq!(compacted.lane_idx, u8::MAX);
}
