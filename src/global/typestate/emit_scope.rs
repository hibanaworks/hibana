use super::{
    builder::encode_typestate_len,
    facts::StateIndex,
    registry::{
        RouteScopeRecord, RouteScopeScratchRecord, SCOPE_LINK_NONE, ScopeRecord, ScopeRegistry,
    },
};
use crate::eff::EffIndex;
use crate::global::const_dsl::{CompactScopeId, ScopeId, ScopeKind};
use crate::global::role_program::LaneWord;

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
    scope_records[idx].parallel_root = match scope_kind {
        ScopeKind::Parallel => idx as u16,
        _ if parent_entry == SCOPE_LINK_NONE => SCOPE_LINK_NONE,
        _ => scope_records[parent_entry as usize].parallel_root,
    };
    scope_records[idx].enclosing_loop = match scope_kind {
        ScopeKind::Loop => idx as u16,
        _ if parent_entry == SCOPE_LINK_NONE => SCOPE_LINK_NONE,
        _ => scope_records[parent_entry as usize].enclosing_loop,
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
    route_offer_lane_words: *mut LaneWord,
    route_arm1_lane_words: *mut LaneWord,
    route_lane_word_len: usize,
    route_records_sparse: *mut RouteScopeScratchRecord,
    lane_slot_count: usize,
    scope_lane_first_eff: *mut EffIndex,
    scope_lane_last_eff: *mut EffIndex,
    route_arm0_lane_last_eff_by_slot: *mut EffIndex,
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
                let sparse = &*route_records_sparse.add(slot_idx);
                let lane_word_start = sparse.lane_word_start();
                let mut record = RouteScopeRecord::EMPTY;
                record.route_recv = sparse.route_recv;
                record.passive_arm_jump = sparse.passive_arm_jump;
                record.offer_lane_word_start = encode_typestate_len(lane_word_start);
                record.offer_entry = sparse.offer_entry;
                record.arm1_lane_word_start = encode_typestate_len(lane_word_start);
                record.first_recv_dispatch = sparse.first_recv_dispatch;
                record.first_recv_len = sparse.first_recv_len;
                record.first_recv_label_mask = sparse.first_recv_label_mask;
                record.first_recv_dispatch_label_mask = sparse.first_recv_dispatch_label_mask;
                record.first_recv_dispatch_arm_mask = sparse.first_recv_dispatch_arm_mask;
                record.first_recv_dispatch_lane_mask = sparse.first_recv_dispatch_lane_mask;
                route_records.add(route_scope_len).write(record);
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
        core::ptr::addr_of_mut!((*dst).len).write(encode_typestate_len(scope_records_len));
        core::ptr::addr_of_mut!((*dst).slots_by_scope).write(slots_by_scope);
        core::ptr::addr_of_mut!((*dst).route_dense_by_slot).write(route_dense_by_slot);
        core::ptr::addr_of_mut!((*dst).route_records).write(route_records);
        core::ptr::addr_of_mut!((*dst).route_scope_len)
            .write(encode_typestate_len(route_scope_len));
        core::ptr::addr_of_mut!((*dst).route_offer_lane_words).write(route_offer_lane_words);
        core::ptr::addr_of_mut!((*dst).route_arm1_lane_words).write(route_arm1_lane_words);
        core::ptr::addr_of_mut!((*dst).route_lane_word_len)
            .write(encode_typestate_len(route_lane_word_len));
        core::ptr::addr_of_mut!((*dst).lane_slot_count)
            .write(encode_typestate_len(lane_slot_count));
        core::ptr::addr_of_mut!((*dst).scope_lane_first_eff).write(scope_lane_first_eff);
        core::ptr::addr_of_mut!((*dst).scope_lane_last_eff).write(scope_lane_last_eff);
        core::ptr::addr_of_mut!((*dst).route_arm0_lane_last_eff_by_slot)
            .write(route_arm0_lane_last_eff_by_slot);
        core::ptr::addr_of_mut!((*dst).frontier_entry_capacity_value).write(0);
    }
    let registry = unsafe { &*dst };
    let frontier_entry_capacity = core::cmp::max(
        core::cmp::min(registry.derive_max_offer_entries(), u8::BITS as usize),
        usize::from(route_scope_len != 0),
    );
    if frontier_entry_capacity > u8::MAX as usize {
        panic!("frontier entry capacity overflow");
    }
    unsafe {
        core::ptr::addr_of_mut!((*dst).frontier_entry_capacity_value)
            .write(frontier_entry_capacity as u8);
    }
}
