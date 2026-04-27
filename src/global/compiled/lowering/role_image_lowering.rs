use crate::global::role_program::{
    LaneSteps, LaneWord, LocalStep, PhaseRouteGuard, lane_word_count,
};
use crate::global::typestate::{LocalAction, RoleTypestateValue, StateIndex};

use super::super::images::role::{
    MACHINE_NO_STEP, PhaseImageHeader, PhaseLaneEntry, encode_compact_count_u16,
    encode_compact_step_index,
};

pub(super) fn build_local_steps_into(
    role: u8,
    typestate: &RoleTypestateValue,
    steps: &mut [LocalStep],
    eff_index_to_step: &mut [u16],
) -> usize {
    let mut idx = 0usize;
    while idx < eff_index_to_step.len() {
        eff_index_to_step[idx] = MACHINE_NO_STEP;
        idx += 1;
    }
    let mut step_idx = 0usize;
    while step_idx < steps.len() {
        steps[step_idx] = LocalStep::EMPTY;
        step_idx += 1;
    }

    let mut node_idx = 0usize;
    while node_idx < typestate.len() {
        match typestate.node(node_idx).action() {
            LocalAction::Send { eff_index, .. } => {
                let idx = eff_index.as_usize();
                if idx >= eff_index_to_step.len() {
                    panic!("local step eff_index exceeds lowering scratch capacity");
                }
                if eff_index_to_step[idx] == MACHINE_NO_STEP {
                    eff_index_to_step[idx] = node_idx as u16;
                }
            }
            LocalAction::Recv { eff_index, .. } => {
                let idx = eff_index.as_usize();
                if idx >= eff_index_to_step.len() {
                    panic!("local step eff_index exceeds lowering scratch capacity");
                }
                if eff_index_to_step[idx] == MACHINE_NO_STEP {
                    eff_index_to_step[idx] = node_idx as u16;
                }
            }
            LocalAction::Local { eff_index, .. } => {
                let idx = eff_index.as_usize();
                if idx >= eff_index_to_step.len() {
                    panic!("local step eff_index exceeds lowering scratch capacity");
                }
                if eff_index_to_step[idx] == MACHINE_NO_STEP {
                    eff_index_to_step[idx] = node_idx as u16;
                }
            }
            LocalAction::Terminate | LocalAction::Jump { .. } => {}
        }
        node_idx += 1;
    }

    let mut len = 0usize;
    let mut idx = 0usize;
    while idx < eff_index_to_step.len() {
        let state_idx = eff_index_to_step[idx];
        if state_idx != MACHINE_NO_STEP {
            if len >= steps.len() {
                panic!("compiled role local step count exceeds lowering scratch capacity");
            }
            if len > u16::MAX as usize {
                panic!("compiled role local step count overflow");
            }
            steps[len] = match typestate.node(state_idx as usize).action() {
                LocalAction::Send {
                    eff_index,
                    peer,
                    label,
                    resource,
                    is_control,
                    shot,
                    lane,
                    ..
                } => LocalStep::send(eff_index, peer, label, resource, is_control, shot, lane),
                LocalAction::Recv {
                    eff_index,
                    peer,
                    label,
                    resource,
                    is_control,
                    shot,
                    lane,
                    ..
                } => LocalStep::recv(eff_index, peer, label, resource, is_control, shot, lane),
                LocalAction::Local {
                    eff_index,
                    label,
                    resource,
                    is_control,
                    shot,
                    lane,
                    ..
                } => LocalStep::local(eff_index, role, label, resource, is_control, shot, lane),
                LocalAction::Terminate | LocalAction::Jump { .. } => {
                    panic!("local step state index must reference a local action")
                }
            };
            eff_index_to_step[idx] = len as u16;
            len += 1;
        }
        idx += 1;
    }
    len
}

pub(super) fn build_step_index_to_state_into(
    typestate: &RoleTypestateValue,
    steps: &[LocalStep],
    len: usize,
    eff_index_to_step: &[u16],
    step_index_to_state: &mut [StateIndex],
) {
    if len > steps.len() || len > step_index_to_state.len() {
        panic!("compiled role step-state lowering exceeds scratch capacity");
    }
    let mut idx = 0usize;
    while idx < step_index_to_state.len() {
        step_index_to_state[idx] = StateIndex::MAX;
        idx += 1;
    }
    let mut node_idx = 0usize;
    while node_idx < typestate.len() {
        match typestate.node(node_idx).action() {
            LocalAction::Send {
                eff_index,
                peer,
                label,
                lane,
                ..
            } => record_step_state(
                steps,
                len,
                eff_index_to_step,
                step_index_to_state,
                node_idx,
                eff_index,
                true,
                false,
                label,
                peer,
                lane,
            ),
            LocalAction::Recv {
                eff_index,
                peer,
                label,
                lane,
                ..
            } => record_step_state(
                steps,
                len,
                eff_index_to_step,
                step_index_to_state,
                node_idx,
                eff_index,
                false,
                false,
                label,
                peer,
                lane,
            ),
            LocalAction::Local {
                eff_index,
                label,
                lane,
                ..
            } => record_step_state(
                steps,
                len,
                eff_index_to_step,
                step_index_to_state,
                node_idx,
                eff_index,
                false,
                true,
                label,
                0,
                lane,
            ),
            LocalAction::Terminate | LocalAction::Jump { .. } => {}
        }
        node_idx += 1;
    }
}

fn record_step_state(
    steps: &[LocalStep],
    len: usize,
    eff_index_to_step: &[u16],
    step_index_to_state: &mut [StateIndex],
    node_idx: usize,
    eff_index: crate::eff::EffIndex,
    is_send: bool,
    is_local: bool,
    label: u8,
    peer: u8,
    lane: u8,
) {
    let eff_idx = eff_index.as_usize();
    if eff_idx >= eff_index_to_step.len() {
        panic!("eff_index out of bounds for compiled role mapping scratch");
    }
    let step_idx = eff_index_to_step[eff_idx];
    if step_idx == MACHINE_NO_STEP {
        return;
    }
    let step_idx = step_idx as usize;
    if step_idx >= len || step_idx >= steps.len() || step_idx >= step_index_to_state.len() {
        panic!("compiled role step index out of bounds");
    }
    let step = steps[step_idx];
    let matches = if is_local {
        step.is_local_action() && step.label() == label && step.lane() == lane
    } else if is_send {
        step.is_send() && step.label() == label && step.peer() == peer && step.lane() == lane
    } else {
        step.is_recv() && step.label() == label && step.peer() == peer && step.lane() == lane
    };
    if !matches {
        panic!("compiled role typestate mapping mismatch");
    }
    let mapped = StateIndex::from_usize(node_idx);
    if step_index_to_state[step_idx].is_max() {
        step_index_to_state[step_idx] = mapped;
    } else if step_index_to_state[step_idx].raw() != mapped.raw() {
        panic!("duplicate typestate mapping for step index");
    }
}

pub(super) unsafe fn build_phase_image_from_steps(
    role: u8,
    steps: &[LocalStep],
    len: usize,
    typestate: &RoleTypestateValue,
    step_index_to_state: &[StateIndex],
    route_guards: &mut [PhaseRouteGuard],
    parallel_ranges: &mut [(usize, usize)],
    phase_headers: *mut PhaseImageHeader,
    phase_header_cap: usize,
    phase_lane_entries: *mut PhaseLaneEntry,
    phase_lane_entry_cap: usize,
    phase_lane_words: *mut LaneWord,
    phase_lane_word_cap: usize,
) -> (usize, usize, usize) {
    if len > steps.len() || len > step_index_to_state.len() || len > route_guards.len() {
        panic!("compiled role phase lowering exceeds scratch capacity");
    }
    unsafe {
        initialize_phase_image_storage(
            phase_headers,
            phase_header_cap,
            phase_lane_entries,
            phase_lane_entry_cap,
            phase_lane_words,
            phase_lane_word_cap,
        );
    }
    let mut range_idx = 0usize;
    while range_idx < parallel_ranges.len() {
        parallel_ranges[range_idx] = (0, 0);
        range_idx += 1;
    }
    if len == 0 {
        return (0, 0, 0);
    }

    build_route_guards_for_steps_into(role, len, typestate, step_index_to_state, route_guards);

    let mut phase_count = 0usize;
    let mut phase_lane_entry_len = 0usize;
    let mut phase_lane_word_len = 0usize;

    if !typestate.has_parallel_phase_scope() {
        unsafe {
            push_phase_range_to_image(
                steps,
                0,
                len,
                route_guards,
                phase_headers,
                phase_header_cap,
                phase_lane_entries,
                phase_lane_entry_cap,
                phase_lane_words,
                phase_lane_word_cap,
                &mut phase_count,
                &mut phase_lane_entry_len,
                &mut phase_lane_word_len,
            );
        }
    } else {
        let mut parallel_count = 0usize;
        loop {
            let Some(range) = typestate.parallel_phase_range_at(parallel_count) else {
                break;
            };
            if parallel_count >= parallel_ranges.len() {
                panic!("compiled role phase capacity exceeded");
            }
            parallel_ranges[parallel_count] = range;
            parallel_count += 1;
        }

        if parallel_count == 0 {
            unsafe {
                push_phase_range_to_image(
                    steps,
                    0,
                    len,
                    route_guards,
                    phase_headers,
                    phase_header_cap,
                    phase_lane_entries,
                    phase_lane_entry_cap,
                    phase_lane_words,
                    phase_lane_word_cap,
                    &mut phase_count,
                    &mut phase_lane_entry_len,
                    &mut phase_lane_word_len,
                );
            }
        } else {
            let mut current_step = 0usize;
            let mut range_idx = 0usize;
            while range_idx < parallel_count {
                let (enter_eff, exit_eff) = parallel_ranges[range_idx];

                let seq_start = current_step;
                let mut seq_end = current_step;
                while seq_end < len && steps[seq_end].eff_index().as_usize() < enter_eff {
                    seq_end += 1;
                }
                if seq_end > seq_start {
                    unsafe {
                        push_phase_range_to_image(
                            steps,
                            seq_start,
                            seq_end,
                            route_guards,
                            phase_headers,
                            phase_header_cap,
                            phase_lane_entries,
                            phase_lane_entry_cap,
                            phase_lane_words,
                            phase_lane_word_cap,
                            &mut phase_count,
                            &mut phase_lane_entry_len,
                            &mut phase_lane_word_len,
                        );
                    }
                }

                let par_start = seq_end;
                let mut par_end = par_start;
                while par_end < len && steps[par_end].eff_index().as_usize() < exit_eff {
                    par_end += 1;
                }
                if par_end > par_start {
                    unsafe {
                        push_phase_range_to_image(
                            steps,
                            par_start,
                            par_end,
                            route_guards,
                            phase_headers,
                            phase_header_cap,
                            phase_lane_entries,
                            phase_lane_entry_cap,
                            phase_lane_words,
                            phase_lane_word_cap,
                            &mut phase_count,
                            &mut phase_lane_entry_len,
                            &mut phase_lane_word_len,
                        );
                    }
                }

                current_step = par_end;
                range_idx += 1;
            }

            if current_step < len {
                unsafe {
                    push_phase_range_to_image(
                        steps,
                        current_step,
                        len,
                        route_guards,
                        phase_headers,
                        phase_header_cap,
                        phase_lane_entries,
                        phase_lane_entry_cap,
                        phase_lane_words,
                        phase_lane_word_cap,
                        &mut phase_count,
                        &mut phase_lane_entry_len,
                        &mut phase_lane_word_len,
                    );
                }
            }

            if phase_count == 0 {
                unsafe {
                    push_phase_range_to_image(
                        steps,
                        0,
                        len,
                        route_guards,
                        phase_headers,
                        phase_header_cap,
                        phase_lane_entries,
                        phase_lane_entry_cap,
                        phase_lane_words,
                        phase_lane_word_cap,
                        &mut phase_count,
                        &mut phase_lane_entry_len,
                        &mut phase_lane_word_len,
                    );
                }
            }
        }
    }

    (phase_count, phase_lane_entry_len, phase_lane_word_len)
}

fn build_route_guards_for_steps_into(
    role: u8,
    len: usize,
    typestate: &RoleTypestateValue,
    step_index_to_state: &[StateIndex],
    route_guards: &mut [PhaseRouteGuard],
) {
    let mut idx = 0usize;
    while idx < route_guards.len() {
        route_guards[idx] = PhaseRouteGuard::EMPTY;
        idx += 1;
    }
    let mut step_idx = 0usize;
    while step_idx < len {
        let state = step_index_to_state[step_idx];
        if let Some((scope, arm)) =
            crate::global::typestate::phase_route_guard_for_state_for_role(typestate, role, state)
        {
            route_guards[step_idx] = PhaseRouteGuard::new(scope, arm);
        }
        step_idx += 1;
    }
}

unsafe fn push_phase_range_to_image(
    steps: &[LocalStep],
    start: usize,
    end: usize,
    route_guards: &[PhaseRouteGuard],
    phase_headers: *mut PhaseImageHeader,
    phase_header_cap: usize,
    phase_lane_entries: *mut PhaseLaneEntry,
    phase_lane_entry_cap: usize,
    phase_lane_words: *mut LaneWord,
    phase_lane_word_cap: usize,
    phase_count: &mut usize,
    total_lane_entries: &mut usize,
    total_lane_words: &mut usize,
) {
    if *phase_count >= phase_header_cap {
        panic!("compiled role phase capacity exceeded");
    }
    let lane_entry_start = *total_lane_entries;
    let lane_word_start = *total_lane_words;
    let mut min_start = u16::MAX;
    let mut phase_lane_entry_len = 0usize;
    let mut max_lane_plus_one = 0usize;
    let mut step_idx = start;
    while step_idx < end {
        let lane = steps[step_idx].lane();
        let lane_plus_one = lane as usize + 1;
        if lane_plus_one > max_lane_plus_one {
            max_lane_plus_one = lane_plus_one;
        }
        let mut entry_idx = 0usize;
        let mut matched = false;
        while entry_idx < phase_lane_entry_len {
            let entry = unsafe { &mut *phase_lane_entries.add(lane_entry_start + entry_idx) };
            if entry.lane == lane {
                if entry.steps.len == u16::MAX {
                    panic!("phase lane length overflow");
                }
                entry.steps.len += 1;
                matched = true;
                break;
            }
            entry_idx += 1;
        }
        if !matched {
            if *total_lane_entries >= phase_lane_entry_cap {
                panic!("compiled role phase lane-entry capacity exceeded");
            }
            let lane_start = encode_compact_step_index(step_idx);
            unsafe {
                phase_lane_entries
                    .add(*total_lane_entries)
                    .write(PhaseLaneEntry {
                        lane,
                        steps: LaneSteps {
                            start: lane_start,
                            len: 1,
                        },
                    });
            }
            *total_lane_entries += 1;
            phase_lane_entry_len += 1;
            if lane_start < min_start {
                min_start = lane_start;
            }
        }
        step_idx += 1;
    }
    let phase_lane_word_len = lane_word_count(max_lane_plus_one);
    if phase_lane_entry_len > u16::MAX as usize {
        panic!("compiled role phase lane-entry count overflow");
    }
    if lane_entry_start > u16::MAX as usize {
        panic!("compiled role phase lane-entry offset overflow");
    }
    if phase_lane_word_len > u16::MAX as usize {
        panic!("compiled role phase lane-word count overflow");
    }
    if lane_word_start > u16::MAX as usize {
        panic!("compiled role phase lane-word offset overflow");
    }
    if lane_word_start.saturating_add(phase_lane_word_len) > phase_lane_word_cap {
        panic!("compiled role phase lane-word capacity exceeded");
    }
    let mut word_idx = 0usize;
    while word_idx < phase_lane_word_len {
        unsafe {
            phase_lane_words.add(lane_word_start + word_idx).write(0);
        }
        word_idx += 1;
    }
    let mut entry_idx = 0usize;
    while entry_idx < phase_lane_entry_len {
        let lane = unsafe { (*phase_lane_entries.add(lane_entry_start + entry_idx)).lane as usize };
        let word_bits = LaneWord::BITS as usize;
        let word_idx = lane / word_bits;
        let bit = 1usize << (lane % word_bits);
        unsafe {
            let slot = phase_lane_words.add(lane_word_start + word_idx);
            slot.write(slot.read() | bit);
        }
        entry_idx += 1;
    }
    unsafe {
        phase_headers.add(*phase_count).write(PhaseImageHeader {
            lane_entry_start: encode_compact_count_u16(lane_entry_start),
            lane_entry_len: encode_compact_count_u16(phase_lane_entry_len),
            lane_word_start: encode_compact_count_u16(lane_word_start),
            lane_word_len: encode_compact_count_u16(phase_lane_word_len),
            min_start: if phase_lane_entry_len == 0 {
                0
            } else {
                min_start
            },
            route_guard: route_guard_for_range(route_guards, start, end),
        });
    }
    *phase_count += 1;
    *total_lane_words += phase_lane_word_len;
}

fn route_guard_for_range(
    route_guards: &[PhaseRouteGuard],
    start: usize,
    end: usize,
) -> PhaseRouteGuard {
    if start >= end || start >= route_guards.len() {
        return PhaseRouteGuard::EMPTY;
    }
    let guard = route_guards[start];
    let mut idx = start + 1;
    while idx < end && idx < route_guards.len() {
        let candidate = route_guards[idx];
        if !guard.matches(candidate) {
            return PhaseRouteGuard::EMPTY;
        }
        idx += 1;
    }
    guard
}

unsafe fn initialize_phase_image_storage(
    phase_headers: *mut PhaseImageHeader,
    phase_header_cap: usize,
    phase_lane_entries: *mut PhaseLaneEntry,
    phase_lane_entry_cap: usize,
    phase_lane_words: *mut LaneWord,
    phase_lane_word_cap: usize,
) {
    let mut phase_idx = 0usize;
    while phase_idx < phase_header_cap {
        unsafe {
            phase_headers.add(phase_idx).write(PhaseImageHeader::EMPTY);
        }
        phase_idx += 1;
    }
    let mut lane_entry_idx = 0usize;
    while lane_entry_idx < phase_lane_entry_cap {
        unsafe {
            phase_lane_entries
                .add(lane_entry_idx)
                .write(PhaseLaneEntry::EMPTY);
        }
        lane_entry_idx += 1;
    }
    let mut lane_word_idx = 0usize;
    while lane_word_idx < phase_lane_word_cap {
        unsafe {
            phase_lane_words.add(lane_word_idx).write(0);
        }
        lane_word_idx += 1;
    }
}
