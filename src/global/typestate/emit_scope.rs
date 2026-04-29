use super::{
    builder::encode_typestate_len,
    facts::StateIndex,
    registry::{
        ROUTE_DISPATCH_SHAPE_NONE, RouteDispatchEntry, RouteDispatchShape, RouteScopeRecord,
        RouteScopeScratchRecord, SCOPE_LINK_NONE, ScopeRecord, ScopeRegistry,
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

pub(super) unsafe fn stream_scope_registry_scope_rows(
    dst: *mut ScopeRegistry,
    scope_records: *mut ScopeRecord,
    slots_by_scope: *mut u16,
    scope_records_len: usize,
) {
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
    }
}

pub(super) unsafe fn stream_scope_registry_route_slot_rows(
    dst: *mut ScopeRegistry,
    scope_records: *mut ScopeRecord,
    route_dense_by_slot: *mut u16,
    route_scope_cap: usize,
    scope_records_len: usize,
) -> usize {
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
            }
            route_scope_len += 1;
        } else {
            unsafe { route_dense_by_slot.add(slot_idx).write(u16::MAX) };
        }
        slot_idx += 1;
    }
    unsafe {
        core::ptr::addr_of_mut!((*dst).route_dense_by_slot).write(route_dense_by_slot);
        core::ptr::addr_of_mut!((*dst).route_scope_len)
            .write(encode_typestate_len(route_scope_len));
    }
    route_scope_len
}

pub(super) unsafe fn stream_scope_registry_lane_mask_rows(
    scope_records: *mut ScopeRecord,
    route_dense_by_slot: *mut u16,
    route_offer_lane_words: *mut LaneWord,
    route_arm0_lane_words: *mut LaneWord,
    route_arm1_lane_words: *mut LaneWord,
    route_lane_word_len: usize,
    route_records_sparse: *mut RouteScopeScratchRecord,
    route_arm0_lane_last_eff_by_slot: *mut EffIndex,
    lane_slot_count: usize,
    scope_records_len: usize,
) {
    let mut slot_idx = 0usize;
    while slot_idx < scope_records_len {
        let record = unsafe { &*scope_records.add(slot_idx) };
        if record.kind == ScopeKind::Route {
            let route_scope_len = unsafe { *route_dense_by_slot.add(slot_idx) as usize };
            let sparse = unsafe { &*route_records_sparse.add(slot_idx) };
            let lane_word_start = sparse.lane_word_start();
            let compact_start = route_scope_len.saturating_mul(route_lane_word_len);
            if route_lane_word_len != 0 {
                unsafe {
                    core::ptr::copy(
                        route_offer_lane_words.add(lane_word_start),
                        route_offer_lane_words.add(compact_start),
                        route_lane_word_len,
                    );
                    core::ptr::copy(
                        route_arm0_lane_words.add(lane_word_start),
                        route_arm0_lane_words.add(compact_start),
                        route_lane_word_len,
                    );
                    core::ptr::copy(
                        route_arm1_lane_words.add(lane_word_start),
                        route_arm1_lane_words.add(compact_start),
                        route_lane_word_len,
                    );
                }
            }
            if lane_slot_count != 0 {
                unsafe {
                    core::ptr::copy(
                        route_arm0_lane_last_eff_by_slot.add(slot_idx * lane_slot_count),
                        route_arm0_lane_last_eff_by_slot.add(route_scope_len * lane_slot_count),
                        lane_slot_count,
                    );
                }
            }
        }
        slot_idx += 1;
    }
}

pub(super) unsafe fn stream_scope_registry_route_record_rows(
    dst: *mut ScopeRegistry,
    scope_records: *mut ScopeRecord,
    route_dense_by_slot: *mut u16,
    route_records: *mut RouteScopeRecord,
    route_offer_lane_words: *mut LaneWord,
    route_arm0_lane_words: *mut LaneWord,
    route_arm1_lane_words: *mut LaneWord,
    route_lane_word_len: usize,
    route_dispatch_shapes: *mut RouteDispatchShape,
    route_dispatch_shape_cap: usize,
    route_dispatch_entries: *mut RouteDispatchEntry,
    route_dispatch_entry_cap: usize,
    route_dispatch_targets: *mut StateIndex,
    route_dispatch_target_cap: usize,
    route_records_sparse: *mut RouteScopeScratchRecord,
    lane_slot_count: usize,
    route_arm0_lane_last_eff_by_slot: *mut EffIndex,
    scope_records_len: usize,
) {
    let mut dispatch_shape_len = 0usize;
    let mut dispatch_entry_len = 0usize;
    let mut dispatch_target_len = 0usize;
    let mut slot_idx = 0usize;
    while slot_idx < scope_records_len {
        let scope_record = unsafe { &*scope_records.add(slot_idx) };
        if scope_record.kind == ScopeKind::Route {
            let route_scope_len = unsafe { *route_dense_by_slot.add(slot_idx) as usize };
            let sparse = unsafe { &*route_records_sparse.add(slot_idx) };
            let dispatch_shape = unsafe {
                intern_route_dispatch_shape(
                    route_dispatch_shapes,
                    &mut dispatch_shape_len,
                    route_dispatch_shape_cap,
                    route_dispatch_entries,
                    &mut dispatch_entry_len,
                    route_dispatch_entry_cap,
                    sparse,
                )
            };
            let dispatch_target_start = unsafe {
                append_route_dispatch_targets(
                    route_dispatch_targets,
                    &mut dispatch_target_len,
                    route_dispatch_target_cap,
                    sparse,
                )
            };
            let lane_word_start = route_scope_len.saturating_mul(route_lane_word_len);
            let mut record = RouteScopeRecord::EMPTY;
            record.route_recv = sparse.route_recv;
            record.passive_arm_jump = sparse.passive_arm_jump;
            record.offer_lane_word_start = encode_typestate_len(lane_word_start);
            record.offer_entry = sparse.offer_entry;
            record.route_arm_lane_word_start = encode_typestate_len(lane_word_start);
            record.dispatch_shape = dispatch_shape;
            record.dispatch_target_start = dispatch_target_start;
            unsafe { route_records.add(route_scope_len).write(record) };
        }
        slot_idx += 1;
    }

    unsafe {
        core::ptr::addr_of_mut!((*dst).route_records).write(route_records);
        core::ptr::addr_of_mut!((*dst).route_offer_lane_words).write(route_offer_lane_words);
        core::ptr::addr_of_mut!((*dst).route_arm0_lane_words).write(route_arm0_lane_words);
        core::ptr::addr_of_mut!((*dst).route_arm1_lane_words).write(route_arm1_lane_words);
        core::ptr::addr_of_mut!((*dst).route_lane_word_len)
            .write(encode_typestate_len(route_lane_word_len));
        core::ptr::addr_of_mut!((*dst).route_dispatch_shapes).write(route_dispatch_shapes);
        core::ptr::addr_of_mut!((*dst).route_dispatch_shape_len)
            .write(encode_typestate_len(dispatch_shape_len));
        core::ptr::addr_of_mut!((*dst).route_dispatch_entries).write(route_dispatch_entries);
        core::ptr::addr_of_mut!((*dst).route_dispatch_entry_len)
            .write(encode_typestate_len(dispatch_entry_len));
        core::ptr::addr_of_mut!((*dst).route_dispatch_targets).write(route_dispatch_targets);
        core::ptr::addr_of_mut!((*dst).route_dispatch_target_len)
            .write(encode_typestate_len(dispatch_target_len));
        core::ptr::addr_of_mut!((*dst).lane_slot_count)
            .write(encode_typestate_len(lane_slot_count));
        core::ptr::addr_of_mut!((*dst).route_arm0_lane_last_eff_by_route)
            .write(route_arm0_lane_last_eff_by_slot);
    }
}

pub(super) unsafe fn finalize_scope_registry_lane_rows(
    dst: *mut ScopeRegistry,
    lane_slot_count: usize,
    scope_lane_first_eff: *mut EffIndex,
    scope_lane_last_eff: *mut EffIndex,
) {
    unsafe {
        core::ptr::addr_of_mut!((*dst).lane_slot_count)
            .write(encode_typestate_len(lane_slot_count));
        core::ptr::addr_of_mut!((*dst).scope_lane_first_eff).write(scope_lane_first_eff);
        core::ptr::addr_of_mut!((*dst).scope_lane_last_eff).write(scope_lane_last_eff);
        core::ptr::addr_of_mut!((*dst).frontier_entry_capacity_value).write(0);
    }
    let registry = unsafe { &*dst };
    let route_scope_len = registry.route_scope_count();
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

#[inline(always)]
unsafe fn intern_route_dispatch_shape(
    shapes: *mut RouteDispatchShape,
    shapes_len: &mut usize,
    shape_cap: usize,
    entries: *mut RouteDispatchEntry,
    entries_len: &mut usize,
    entry_cap: usize,
    sparse: &RouteScopeScratchRecord,
) -> u16 {
    if sparse.first_recv_len == 0 {
        return ROUTE_DISPATCH_SHAPE_NONE;
    }
    let sparse_len = sparse.first_recv_len as usize;
    let mut shape_idx = 0usize;
    while shape_idx < *shapes_len {
        let shape = unsafe { &*shapes.add(shape_idx) };
        if shape.first_recv_frame_label_mask != sparse.first_recv_frame_label_mask
            || shape.first_recv_dispatch_arm_frame_label_masks
                != sparse.first_recv_dispatch_arm_frame_label_masks
            || shape.first_recv_dispatch_arm_mask != sparse.first_recv_dispatch_arm_mask
            || shape.first_recv_dispatch_lane_mask != sparse.first_recv_dispatch_lane_mask
            || shape.entries_len as usize != sparse_len
        {
            shape_idx += 1;
            continue;
        }
        let mut entry_idx = 0usize;
        let mut matched = true;
        while entry_idx < sparse_len {
            let existing = unsafe { *entries.add(shape.entries_start as usize + entry_idx) };
            let (frame_label, lane, arm, _) = sparse.first_recv_dispatch[entry_idx];
            if existing.frame_label != frame_label || existing.lane != lane || existing.arm != arm {
                matched = false;
                break;
            }
            entry_idx += 1;
        }
        if matched {
            return encode_typestate_len(shape_idx);
        }
        shape_idx += 1;
    }

    if *shapes_len >= shape_cap {
        panic!("route dispatch shape registry overflow");
    }
    if entries_len.saturating_add(sparse_len) > entry_cap {
        panic!("route dispatch entry registry overflow");
    }
    let entries_start = *entries_len;
    let mut entry_idx = 0usize;
    while entry_idx < sparse_len {
        let (frame_label, lane, arm, _) = sparse.first_recv_dispatch[entry_idx];
        unsafe {
            entries
                .add(entries_start + entry_idx)
                .write(RouteDispatchEntry {
                    frame_label,
                    lane,
                    arm,
                });
        }
        entry_idx += 1;
    }
    *entries_len += sparse_len;
    unsafe {
        shapes.add(*shapes_len).write(RouteDispatchShape {
            first_recv_frame_label_mask: sparse.first_recv_frame_label_mask,
            first_recv_dispatch_arm_frame_label_masks: sparse
                .first_recv_dispatch_arm_frame_label_masks,
            entries_start: encode_typestate_len(entries_start),
            entries_len: sparse.first_recv_len,
            first_recv_dispatch_arm_mask: sparse.first_recv_dispatch_arm_mask,
            first_recv_dispatch_lane_mask: sparse.first_recv_dispatch_lane_mask,
        });
    }
    let idx = *shapes_len;
    *shapes_len += 1;
    encode_typestate_len(idx)
}

#[inline(always)]
unsafe fn append_route_dispatch_targets(
    targets: *mut StateIndex,
    targets_len: &mut usize,
    target_cap: usize,
    sparse: &RouteScopeScratchRecord,
) -> u16 {
    let sparse_len = sparse.first_recv_len as usize;
    if sparse_len == 0 {
        return 0;
    }
    if targets_len.saturating_add(sparse_len) > target_cap {
        panic!("route dispatch target registry overflow");
    }
    let start = *targets_len;
    let mut idx = 0usize;
    while idx < sparse_len {
        let (_, _, _, target) = sparse.first_recv_dispatch[idx];
        unsafe {
            targets.add(start + idx).write(target);
        }
        idx += 1;
    }
    *targets_len += sparse_len;
    encode_typestate_len(start)
}
