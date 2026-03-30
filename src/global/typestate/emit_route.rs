use super::{
    facts::{StateIndex, state_index_to_usize},
    registry::ScopeRecord,
};
use crate::{
    eff,
    global::const_dsl::{ScopeId, ScopeKind},
};

pub(super) const MAX_LOOP_TRACKED: usize = eff::meta::MAX_EFF_NODES;

pub(super) const fn find_loop_entry_state(
    ids: &[ScopeId; MAX_LOOP_TRACKED],
    states: &[Option<StateIndex>; MAX_LOOP_TRACKED],
    len: usize,
    scope_id: ScopeId,
) -> Option<StateIndex> {
    let mut idx = 0usize;
    while idx < len {
        if ids[idx].raw() == scope_id.raw() {
            return states[idx];
        }
        idx += 1;
    }
    None
}

pub(super) const fn store_loop_entry_if_absent(
    ids: &mut [ScopeId; MAX_LOOP_TRACKED],
    states: &mut [Option<StateIndex>; MAX_LOOP_TRACKED],
    len: &mut usize,
    scope_id: ScopeId,
    state: StateIndex,
) {
    let mut idx = 0usize;
    while idx < *len {
        if ids[idx].raw() == scope_id.raw() {
            if states[idx].is_none() {
                states[idx] = Some(state);
            }
            return;
        }
        idx += 1;
    }
    if *len >= MAX_LOOP_TRACKED {
        panic!("loop entry table capacity exceeded");
    }
    ids[*len] = scope_id;
    states[*len] = Some(state);
    *len += 1;
}

pub(super) const fn parallel_phase_eff_range(record: ScopeRecord) -> Option<(usize, usize)> {
    let mut min_eff = usize::MAX;
    let mut max_eff = 0usize;
    let mut have_lane = false;
    let mut lane_idx = 0usize;
    while lane_idx < crate::global::role_program::MAX_LANES {
        let first = record.lane_first_eff[lane_idx];
        if first.raw() != crate::eff::EffIndex::MAX.raw() {
            let first_idx = first.as_usize();
            let last = record.lane_last_eff[lane_idx];
            if last.raw() == crate::eff::EffIndex::MAX.raw() {
                panic!("parallel scope lane missing last eff index");
            }
            let last_idx = last.as_usize();
            if !have_lane || first_idx < min_eff {
                min_eff = first_idx;
            }
            if !have_lane || last_idx > max_eff {
                max_eff = last_idx;
            }
            have_lane = true;
        }
        lane_idx += 1;
    }
    if !have_lane {
        None
    } else {
        Some((min_eff, max_eff + 1))
    }
}

pub(super) const fn phase_route_entry_for_arm<const ROLE: u8>(
    record: ScopeRecord,
    arm: usize,
) -> StateIndex {
    let is_controller = match record.controller_role {
        Some(role) => role == ROLE,
        None => true,
    };
    if is_controller {
        record.controller_arm_entry[arm]
    } else {
        record.passive_arm_entry[arm]
    }
}

pub(super) const fn phase_route_arm_for_record<const ROLE: u8>(
    record: ScopeRecord,
    state_idx: usize,
) -> Option<u8> {
    if !matches!(record.kind, ScopeKind::Route) {
        return None;
    }
    let arm0_entry = phase_route_entry_for_arm::<ROLE>(record, 0);
    let arm1_entry = phase_route_entry_for_arm::<ROLE>(record, 1);

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
