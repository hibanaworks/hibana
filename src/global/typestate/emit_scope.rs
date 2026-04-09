use super::{
    facts::StateIndex,
    registry::{RouteScopeRecord, SCOPE_LINK_NONE, ScopeRecord, ScopeRegistry},
};
use crate::global::const_dsl::{CompactScopeId, ScopeId, ScopeKind};

#[inline(never)]
pub(super) const fn alloc_scope_record(
    scope_records: &mut [ScopeRecord],
    scope_records_len: &mut usize,
    scope_range_counter: &mut u16,
    scope: ScopeId,
    scope_kind: ScopeKind,
    linger: bool,
    parent_entry: u16,
    nest: usize,
) -> (usize, bool) {
    let target_raw = scope.canonical().raw();
    let mut idx = 0usize;
    while idx < *scope_records_len {
        let record = &scope_records[idx];
        if record.scope_id.canonical().raw() == target_raw {
            return (idx, false);
        }
        idx += 1;
    }

    if *scope_records_len >= scope_records.len() {
        panic!("structured scope metadata overflow");
    }
    if *scope_range_counter == u16::MAX {
        panic!("scope range ordinal overflow");
    }
    let idx = *scope_records_len;
    scope_records[idx] = ScopeRecord::EMPTY;
    scope_records[idx].scope_id = CompactScopeId::from_scope_id(scope);
    scope_records[idx].kind = scope_kind;
    scope_records[idx].start = StateIndex::MAX;
    scope_records[idx].end = StateIndex::MAX;
    scope_records[idx].linger = linger;
    scope_records[idx].parent = if parent_entry == SCOPE_LINK_NONE {
        SCOPE_LINK_NONE
    } else {
        parent_entry
    };
    scope_records[idx].range = *scope_range_counter;
    scope_records[idx].nest = nest as u16;
    *scope_range_counter = scope_range_counter.wrapping_add(1);
    *scope_records_len += 1;
    (idx, true)
}

pub(super) unsafe fn init_scope_registry(
    dst: *mut ScopeRegistry,
    scope_records: *mut ScopeRecord,
    slots_by_scope: *mut u16,
    route_dense_by_slot: *mut u16,
    route_records: *mut RouteScopeRecord,
    route_scope_cap: usize,
    route_records_sparse: *mut RouteScopeRecord,
    scope_records_len: usize,
) {
    let mut route_scope_len = 0usize;
    let mut slot_idx = 0usize;
    while slot_idx < scope_records_len {
        let record = unsafe { &*scope_records.add(slot_idx) };
        if record.kind == ScopeKind::Route {
            if route_scope_len >= route_scope_cap {
                panic!("route scope registry overflow");
            }
            unsafe {
                route_dense_by_slot
                    .add(slot_idx)
                    .write(route_scope_len as u16);
                route_records
                    .add(route_scope_len)
                    .write(*route_records_sparse.add(slot_idx));
            }
            route_scope_len += 1;
        } else {
            unsafe { route_dense_by_slot.add(slot_idx).write(u16::MAX) };
        }
        slot_idx += 1;
    }

    let mut insert_idx = 0usize;
    while insert_idx < scope_records_len {
        let target_raw = unsafe { (*scope_records.add(insert_idx)).scope_id.canonical().raw() };
        let mut slot = insert_idx;
        while slot > 0 {
            let prev = unsafe { *slots_by_scope.add(slot - 1) };
            let prev_raw = unsafe {
                (*scope_records.add(prev as usize))
                    .scope_id
                    .canonical()
                    .raw()
            };
            if prev_raw <= target_raw {
                break;
            }
            unsafe { slots_by_scope.add(slot).write(prev) };
            slot -= 1;
        }
        unsafe { slots_by_scope.add(slot).write(insert_idx as u16) };
        insert_idx += 1;
    }

    unsafe {
        core::ptr::addr_of_mut!((*dst).records).write(scope_records);
        core::ptr::addr_of_mut!((*dst).len).write(scope_records_len as u16);
        core::ptr::addr_of_mut!((*dst).slots_by_scope).write(slots_by_scope);
        core::ptr::addr_of_mut!((*dst).route_dense_by_slot).write(route_dense_by_slot);
        core::ptr::addr_of_mut!((*dst).route_records).write(route_records);
        core::ptr::addr_of_mut!((*dst).route_scope_len).write(route_scope_len as u16);
    }
}
