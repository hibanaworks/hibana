use super::{
    facts::{StateIndex, state_index_to_usize},
    registry::{ScopeRecord, ScopeRegistry},
};
use crate::{
    eff,
    global::const_dsl::{ScopeId, ScopeKind},
};

pub(super) const MAX_LOOP_TRACKED: usize = eff::meta::MAX_EFF_NODES;

#[inline(never)]
pub(super) const fn find_loop_entry_state(
    ids: &[ScopeId; MAX_LOOP_TRACKED],
    states: &[StateIndex; MAX_LOOP_TRACKED],
    len: usize,
    scope_id: ScopeId,
) -> Option<StateIndex> {
    let mut idx = 0usize;
    while idx < len {
        if ids[idx].raw() == scope_id.raw() {
            return Some(states[idx]);
        }
        idx += 1;
    }
    None
}

#[inline(never)]
pub(super) const fn store_loop_entry_if_absent(
    ids: &mut [ScopeId; MAX_LOOP_TRACKED],
    states: &mut [StateIndex; MAX_LOOP_TRACKED],
    len: &mut usize,
    scope_id: ScopeId,
    state: StateIndex,
) {
    let mut idx = 0usize;
    while idx < *len {
        if ids[idx].raw() == scope_id.raw() {
            return;
        }
        idx += 1;
    }
    if *len >= MAX_LOOP_TRACKED {
        panic!("loop entry table capacity exceeded");
    }
    ids[*len] = scope_id;
    states[*len] = state;
    *len += 1;
}

pub(super) fn parallel_phase_eff_range(
    scope_registry: &ScopeRegistry,
    slot: usize,
    _record: &ScopeRecord,
) -> Option<(usize, usize)> {
    let lane_first_eff = scope_registry.scope_lane_first_row(slot);
    let lane_last_eff = scope_registry.scope_lane_last_row(slot);
    let mut min_eff = usize::MAX;
    let mut max_eff = 0usize;
    let mut have_lane = false;
    let mut lane = 0usize;
    while lane < lane_first_eff.len() {
        let first_eff = lane_first_eff[lane];
        let last_eff = lane_last_eff[lane];
        if first_eff != crate::eff::EffIndex::MAX {
            if last_eff == crate::eff::EffIndex::MAX {
                panic!("parallel scope lane missing last eff index");
            }
            let first_idx = first_eff.dense_ordinal();
            let last_idx = last_eff.dense_ordinal();
            if !have_lane || first_idx < min_eff {
                min_eff = first_idx;
            }
            if !have_lane || last_idx > max_eff {
                max_eff = last_idx;
            }
            have_lane = true;
        }
        lane += 1;
    }
    if have_lane {
        Some((min_eff, max_eff + 1))
    } else {
        None
    }
}

pub(super) const fn phase_route_entry_for_arm(
    record: &ScopeRecord,
    _role: u8,
    arm: usize,
) -> StateIndex {
    record.arm_entry[arm]
}

pub(super) const fn phase_route_arm_for_record(
    record: &ScopeRecord,
    role: u8,
    state_idx: usize,
) -> Option<u8> {
    if !matches!(record.kind, ScopeKind::Route) {
        return None;
    }
    let arm0_entry = phase_route_entry_for_arm(record, role, 0);
    let arm1_entry = phase_route_entry_for_arm(record, role, 1);

    let mut selected_arm = None;
    let mut selected_entry = 0usize;

    if !arm0_entry.is_max() {
        let arm0_idx = state_index_to_usize(arm0_entry);
        if arm0_idx <= state_idx {
            selected_arm = Some(0);
            selected_entry = arm0_idx;
        }
    }

    if !arm1_entry.is_max() {
        let arm1_idx = state_index_to_usize(arm1_entry);
        if arm1_idx <= state_idx && (selected_arm.is_none() || arm1_idx > selected_entry) {
            selected_arm = Some(1);
        }
    }

    selected_arm
}
