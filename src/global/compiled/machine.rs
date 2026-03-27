use crate::global::{
    const_dsl::EffList,
    role_program::{
        LaneSteps, LocalStep, MAX_LANES, MAX_PHASES, MAX_STEPS, Phase, PhaseRouteGuard,
        ProjectedRoleLayout,
    },
    typestate::{LocalAction, RoleTypestate, StateIndex},
};
#[cfg(test)]
use crate::global::role_program::ProjectedRoleData;

const MACHINE_NO_STEP: u16 = u16::MAX;

/// Crate-private owner for a lowered role machine.
#[derive(Clone, Copy)]
pub(crate) struct RoleMachine<const ROLE: u8> {
    layout: ProjectedRoleLayout,
    typestate: RoleTypestate<ROLE>,
    eff_index_to_step: [u16; MAX_STEPS],
    step_index_to_state: [StateIndex; MAX_STEPS],
}

impl<const ROLE: u8> RoleMachine<ROLE> {
    #[inline(always)]
    pub(in crate::global) const fn from_eff_list(eff_list: &EffList) -> Self {
        let typestate = RoleTypestate::<ROLE>::from_program(eff_list);
        let (steps, len, eff_index_to_step) = Self::build_local_steps(&typestate);
        let step_index_to_state =
            Self::build_step_index_to_state(&typestate, &steps, len, &eff_index_to_step);
        let (phases, phase_len) =
            Self::build_phases(&steps, len, &typestate, &step_index_to_state);
        let layout = ProjectedRoleLayout::new(steps, len, phases, phase_len);
        Self {
            layout,
            typestate,
            eff_index_to_step,
            step_index_to_state,
        }
    }

    #[inline(always)]
    pub(in crate::global) const fn validate(eff_list: &EffList) {
        let _ = Self::from_eff_list(eff_list);
    }

    #[inline(always)]
    pub(crate) const fn layout(&self) -> &ProjectedRoleLayout {
        &self.layout
    }

    #[inline(always)]
    pub(crate) const fn typestate(&self) -> &RoleTypestate<ROLE> {
        &self.typestate
    }

    #[inline(always)]
    pub(crate) const fn eff_index_to_step(&self) -> &[u16; MAX_STEPS] {
        &self.eff_index_to_step
    }

    #[inline(always)]
    pub(crate) const fn step_index_to_state(&self) -> &[StateIndex; MAX_STEPS] {
        &self.step_index_to_state
    }

    #[cfg(test)]
    #[inline(always)]
    pub(crate) const fn into_projection(self) -> ProjectedRoleData<ROLE> {
        ProjectedRoleData::new(self.layout, self.typestate)
    }

    pub(crate) fn active_lanes(&self) -> [bool; MAX_LANES] {
        let mut active = [false; MAX_LANES];
        for phase in self.layout().phases() {
            for lane_idx in 0..MAX_LANES {
                if phase.lanes[lane_idx].is_active() {
                    active[lane_idx] = true;
                }
            }
        }
        active
    }

    const fn build_local_steps(
        typestate: &RoleTypestate<ROLE>,
    ) -> ([LocalStep; MAX_STEPS], usize, [u16; MAX_STEPS]) {
        let mut by_eff_index = [LocalStep::EMPTY; MAX_STEPS];
        let mut present = [false; MAX_STEPS];
        let mut node_idx = 0usize;
        while node_idx < typestate.len() {
            match typestate.node(node_idx).action() {
                LocalAction::Send {
                    eff_index,
                    peer,
                    label,
                    resource,
                    is_control,
                    shot,
                    lane,
                    ..
                } => {
                    let idx = eff_index.as_usize();
                    if idx >= MAX_STEPS {
                        panic!("local step eff_index exceeds MAX_STEPS");
                    }
                    if !present[idx] {
                        by_eff_index[idx] = LocalStep::send(
                            eff_index, peer, label, resource, is_control, shot, lane,
                        );
                        present[idx] = true;
                    }
                }
                LocalAction::Recv {
                    eff_index,
                    peer,
                    label,
                    resource,
                    is_control,
                    shot,
                    lane,
                    ..
                } => {
                    let idx = eff_index.as_usize();
                    if idx >= MAX_STEPS {
                        panic!("local step eff_index exceeds MAX_STEPS");
                    }
                    if !present[idx] {
                        by_eff_index[idx] = LocalStep::recv(
                            eff_index, peer, label, resource, is_control, shot, lane,
                        );
                        present[idx] = true;
                    }
                }
                LocalAction::Local {
                    eff_index,
                    label,
                    resource,
                    is_control,
                    shot,
                    lane,
                    ..
                } => {
                    let idx = eff_index.as_usize();
                    if idx >= MAX_STEPS {
                        panic!("local step eff_index exceeds MAX_STEPS");
                    }
                    if !present[idx] {
                        by_eff_index[idx] = LocalStep::local(
                            eff_index, ROLE, label, resource, is_control, shot, lane,
                        );
                        present[idx] = true;
                    }
                }
                LocalAction::None | LocalAction::Terminate | LocalAction::Jump { .. } => {}
            }
            node_idx += 1;
        }

        let mut steps = [LocalStep::EMPTY; MAX_STEPS];
        let mut eff_index_to_step = [MACHINE_NO_STEP; MAX_STEPS];
        let mut len = 0usize;
        let mut idx = 0usize;
        while idx < MAX_STEPS {
            if present[idx] {
                steps[len] = by_eff_index[idx];
                eff_index_to_step[idx] = len as u16;
                len += 1;
            }
            idx += 1;
        }

        (steps, len, eff_index_to_step)
    }

    const fn build_step_index_to_state(
        typestate: &RoleTypestate<ROLE>,
        steps: &[LocalStep; MAX_STEPS],
        len: usize,
        eff_index_to_step: &[u16; MAX_STEPS],
    ) -> [StateIndex; MAX_STEPS] {
        let mut step_index_to_state = [StateIndex::MAX; MAX_STEPS];
        let mut node_idx = 0usize;
        while node_idx < typestate.len() {
            match typestate.node(node_idx).action() {
                LocalAction::Send {
                    eff_index,
                    peer,
                    label,
                    lane,
                    ..
                } => Self::record_step_state(
                    steps,
                    len,
                    eff_index_to_step,
                    &mut step_index_to_state,
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
                } => Self::record_step_state(
                    steps,
                    len,
                    eff_index_to_step,
                    &mut step_index_to_state,
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
                } => Self::record_step_state(
                    steps,
                    len,
                    eff_index_to_step,
                    &mut step_index_to_state,
                    node_idx,
                    eff_index,
                    false,
                    true,
                    label,
                    0,
                    lane,
                ),
                LocalAction::None | LocalAction::Terminate | LocalAction::Jump { .. } => {}
            }
            node_idx += 1;
        }
        step_index_to_state
    }

    const fn record_step_state(
        steps: &[LocalStep; MAX_STEPS],
        len: usize,
        eff_index_to_step: &[u16; MAX_STEPS],
        step_index_to_state: &mut [StateIndex; MAX_STEPS],
        node_idx: usize,
        eff_index: crate::eff::EffIndex,
        is_send: bool,
        is_local: bool,
        label: u8,
        peer: u8,
        lane: u8,
    ) {
        let eff_idx = eff_index.as_usize();
        if eff_idx >= MAX_STEPS {
            panic!("eff_index out of bounds for role machine mapping");
        }
        let step_idx = eff_index_to_step[eff_idx];
        if step_idx == MACHINE_NO_STEP {
            return;
        }
        let step_idx = step_idx as usize;
        if step_idx >= len {
            panic!("role machine step index out of bounds");
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
            panic!("role machine typestate mapping mismatch");
        }
        let mapped = StateIndex::from_usize(node_idx);
        if step_index_to_state[step_idx].is_max() {
            step_index_to_state[step_idx] = mapped;
        } else if step_index_to_state[step_idx].raw() != mapped.raw() {
            panic!("duplicate typestate mapping for step index");
        }
    }

    const fn build_phases(
        steps: &[LocalStep; MAX_STEPS],
        len: usize,
        typestate: &RoleTypestate<ROLE>,
        step_index_to_state: &[StateIndex; MAX_STEPS],
    ) -> ([Phase; MAX_PHASES], usize) {
        let phases = [Phase::EMPTY; MAX_PHASES];

        if len == 0 {
            return (phases, 0);
        }

        let has_parallel = typestate.has_parallel_phase_scope();
        let route_guards = Self::build_route_guards_for_steps(len, typestate, step_index_to_state);

        if !has_parallel {
            return Self::build_single_phase(steps, len, &route_guards);
        }

        Self::build_phases_with_parallel(steps, len, typestate, &route_guards)
    }

    const fn build_route_guards_for_steps(
        len: usize,
        typestate: &RoleTypestate<ROLE>,
        step_index_to_state: &[StateIndex; MAX_STEPS],
    ) -> [PhaseRouteGuard; MAX_STEPS] {
        let mut guards = [PhaseRouteGuard::EMPTY; MAX_STEPS];
        let mut step_idx = 0usize;
        while step_idx < len {
            let state = step_index_to_state[step_idx];
            if let Some((scope, arm)) = typestate.phase_route_guard_for_state(state) {
                guards[step_idx] = PhaseRouteGuard { scope, arm };
            }
            step_idx += 1;
        }
        guards
    }

    const fn build_single_phase(
        steps: &[LocalStep; MAX_STEPS],
        len: usize,
        route_guards: &[PhaseRouteGuard; MAX_STEPS],
    ) -> ([Phase; MAX_PHASES], usize) {
        let mut phases = [Phase::EMPTY; MAX_PHASES];
        let mut lane_lens = [0usize; MAX_LANES];
        let mut lane_first = [usize::MAX; MAX_LANES];

        let mut i = 0;
        while i < len {
            let lane = steps[i].lane() as usize;
            if lane < MAX_LANES {
                if lane_first[lane] == usize::MAX {
                    lane_first[lane] = i;
                }
                lane_lens[lane] += 1;
            }
            i += 1;
        }

        let mut phase = Phase::EMPTY;
        let mut lane_mask = 0u8;
        let mut min_start = usize::MAX;
        let mut lane_idx = 0;
        while lane_idx < MAX_LANES {
            if lane_lens[lane_idx] > 0 {
                let start = lane_first[lane_idx];
                phase.lanes[lane_idx] = LaneSteps {
                    start,
                    len: lane_lens[lane_idx],
                };
                lane_mask |= 1u8 << (lane_idx as u32);
                if start < min_start {
                    min_start = start;
                }
            }
            lane_idx += 1;
        }
        phase.lane_mask = lane_mask;
        phase.min_start = if lane_mask == 0 { 0 } else { min_start };
        phase.route_guard = Self::route_guard_for_range(route_guards, 0, len);

        phases[0] = phase;
        (phases, 1)
    }

    const fn build_phases_with_parallel(
        steps: &[LocalStep; MAX_STEPS],
        len: usize,
        typestate: &RoleTypestate<ROLE>,
        route_guards: &[PhaseRouteGuard; MAX_STEPS],
    ) -> ([Phase; MAX_PHASES], usize) {
        let mut phases = [Phase::EMPTY; MAX_PHASES];
        let mut phase_count = 0usize;

        let mut parallel_ranges = [(0usize, 0usize); MAX_PHASES];
        let mut parallel_count = 0usize;
        while parallel_count < MAX_PHASES {
            let Some(range) = typestate.parallel_phase_range_at(parallel_count) else {
                break;
            };
            parallel_ranges[parallel_count] = range;
            parallel_count += 1;
        }

        if parallel_count == 0 {
            return Self::build_single_phase(steps, len, route_guards);
        }

        let mut current_step = 0usize;

        let mut range_idx = 0;
        while range_idx < parallel_count {
            let (enter_eff, exit_eff) = parallel_ranges[range_idx];

            let seq_start = current_step;
            let mut seq_end = current_step;
            while seq_end < len && steps[seq_end].eff_index().as_usize() < enter_eff {
                seq_end += 1;
            }

            if seq_end > seq_start && phase_count < MAX_PHASES {
                phases[phase_count] =
                    Self::build_phase_for_range(steps, seq_start, seq_end, route_guards);
                phase_count += 1;
            }

            let par_start = seq_end;
            let mut par_end = par_start;
            while par_end < len && steps[par_end].eff_index().as_usize() < exit_eff {
                par_end += 1;
            }

            if par_end > par_start && phase_count < MAX_PHASES {
                phases[phase_count] =
                    Self::build_phase_for_range(steps, par_start, par_end, route_guards);
                phase_count += 1;
            }

            current_step = par_end;
            range_idx += 1;
        }

        if current_step < len && phase_count < MAX_PHASES {
            phases[phase_count] =
                Self::build_phase_for_range(steps, current_step, len, route_guards);
            phase_count += 1;
        }

        if phase_count == 0 {
            return Self::build_single_phase(steps, len, route_guards);
        }

        (phases, phase_count)
    }

    const fn build_phase_for_range(
        steps: &[LocalStep; MAX_STEPS],
        start: usize,
        end: usize,
        route_guards: &[PhaseRouteGuard; MAX_STEPS],
    ) -> Phase {
        let mut phase = Phase::EMPTY;
        let mut lane_lens = [0usize; MAX_LANES];
        let mut lane_first = [usize::MAX; MAX_LANES];

        let mut i = start;
        while i < end {
            let lane = steps[i].lane() as usize;
            if lane < MAX_LANES {
                if lane_first[lane] == usize::MAX {
                    lane_first[lane] = i;
                }
                lane_lens[lane] += 1;
            }
            i += 1;
        }

        let mut lane_mask = 0u8;
        let mut min_start = usize::MAX;
        let mut lane_idx = 0;
        while lane_idx < MAX_LANES {
            if lane_lens[lane_idx] > 0 {
                let start = lane_first[lane_idx];
                phase.lanes[lane_idx] = LaneSteps {
                    start,
                    len: lane_lens[lane_idx],
                };
                lane_mask |= 1u8 << (lane_idx as u32);
                if start < min_start {
                    min_start = start;
                }
            }
            lane_idx += 1;
        }
        phase.lane_mask = lane_mask;
        phase.min_start = if lane_mask == 0 { 0 } else { min_start };
        phase.route_guard = Self::route_guard_for_range(route_guards, start, end);

        phase
    }

    const fn route_guard_for_range(
        route_guards: &[PhaseRouteGuard; MAX_STEPS],
        start: usize,
        end: usize,
    ) -> PhaseRouteGuard {
        if start >= end || start >= MAX_STEPS {
            return PhaseRouteGuard::EMPTY;
        }
        let guard = route_guards[start];
        let mut idx = start + 1;
        while idx < end && idx < MAX_STEPS {
            let candidate = route_guards[idx];
            if !guard.matches(candidate) {
                return PhaseRouteGuard::EMPTY;
            }
            idx += 1;
        }
        guard
    }
}
