use core::ptr;

use crate::global::{
    const_dsl::ScopeId,
    role_program::{
        LaneSteps, LocalStep, MAX_LANES, MAX_PHASES, MAX_STEPS, Phase, PhaseRouteGuard,
        ProjectedRoleLayout,
    },
    typestate::{
        ARM_SHARED, LocalAction, MAX_FIRST_RECV_DISPATCH, RoleTypestate, RoleTypestateValue,
        StateIndex,
    },
};

use super::LoweringSummary;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ControllerArmEntry {
    pub scope: ScopeId,
    pub arm: u8,
    pub entry: StateIndex,
    pub label: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ControllerArmTable {
    entries: [ControllerArmEntry; Self::MAX_ENTRIES],
    len: usize,
}

impl ControllerArmTable {
    const MAX_ENTRIES: usize = crate::global::const_dsl::ScopeId::ORDINAL_CAPACITY as usize * 2;
    const EMPTY_ENTRY: ControllerArmEntry = ControllerArmEntry {
        scope: ScopeId::none(),
        arm: 0,
        entry: StateIndex::MAX,
        label: 0,
    };

    const EMPTY: Self = Self {
        entries: [Self::EMPTY_ENTRY; Self::MAX_ENTRIES],
        len: 0,
    };

    #[inline(always)]
    pub(crate) const fn entry_by_arm(&self, scope: ScopeId, arm: u8) -> Option<(StateIndex, u8)> {
        let mut idx = 0usize;
        while idx < self.len {
            let entry = self.entries[idx];
            if entry.scope.raw() == scope.raw() && entry.arm == arm {
                return Some((entry.entry, entry.label));
            }
            idx += 1;
        }
        None
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FirstRecvDispatchEntry {
    pub scope: ScopeId,
    pub label: u8,
    pub arm: u8,
    pub target: StateIndex,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FirstRecvDispatchTable {
    entries: [FirstRecvDispatchEntry; Self::MAX_ENTRIES],
    len: usize,
}

impl FirstRecvDispatchTable {
    const MAX_ENTRIES: usize =
        crate::global::const_dsl::ScopeId::ORDINAL_CAPACITY as usize * MAX_FIRST_RECV_DISPATCH;
    const EMPTY_ENTRY: FirstRecvDispatchEntry = FirstRecvDispatchEntry {
        scope: ScopeId::none(),
        label: 0,
        arm: ARM_SHARED,
        target: StateIndex::MAX,
    };

    const EMPTY: Self = Self {
        entries: [Self::EMPTY_ENTRY; Self::MAX_ENTRIES],
        len: 0,
    };

    #[inline(always)]
    pub(crate) const fn entry(&self, scope: ScopeId, idx: usize) -> Option<(u8, u8, StateIndex)> {
        let mut scope_idx = 0usize;
        let mut table_idx = 0usize;
        while table_idx < self.len {
            let entry = self.entries[table_idx];
            if entry.scope.raw() == scope.raw() {
                if scope_idx == idx {
                    return Some((entry.label, entry.arm, entry.target));
                }
                scope_idx += 1;
            }
            table_idx += 1;
        }
        None
    }
}

const MACHINE_NO_STEP: u16 = u16::MAX;

/// Crate-private owner for lowered role-local facts.
#[derive(Clone, Debug)]
pub(crate) struct CompiledRole {
    role: u8,
    layout: ProjectedRoleLayout,
    typestate: RoleTypestateValue,
    eff_index_to_step: [u16; MAX_STEPS],
    step_index_to_state: [StateIndex; MAX_STEPS],
    active_lanes: [bool; MAX_LANES],
    controller_arm_table: ControllerArmTable,
    first_recv_dispatch: FirstRecvDispatchTable,
}

impl CompiledRole {
    #[inline(always)]
    #[cfg(test)]
    pub(crate) fn compile<const ROLE: u8>(eff_list: &crate::global::const_dsl::EffList) -> Self {
        let summary = LoweringSummary::scan_const(eff_list);
        Self::from_summary::<ROLE>(&summary)
    }

    #[inline(always)]
    #[cfg(test)]
    pub(crate) fn from_summary<const ROLE: u8>(summary: &LoweringSummary) -> Self {
        let mut compiled = core::mem::MaybeUninit::<Self>::uninit();
        unsafe {
            Self::init_from_summary::<ROLE>(compiled.as_mut_ptr(), summary);
            compiled.assume_init()
        }
    }

    #[inline(always)]
    pub(crate) unsafe fn init_from_summary<const ROLE: u8>(
        dst: *mut Self,
        summary: &LoweringSummary,
    ) {
        let typed_typestate = RoleTypestate::<ROLE>::from_summary(summary);
        let (steps, len, eff_index_to_step) = Self::build_local_steps::<ROLE>(&typed_typestate);
        let step_index_to_state = Self::build_step_index_to_state::<ROLE>(
            &typed_typestate,
            &steps,
            len,
            &eff_index_to_step,
        );
        let (phases, phase_len) =
            Self::build_phases::<ROLE>(&steps, len, &typed_typestate, &step_index_to_state);
        let active_lanes = Self::build_active_lanes_from_phases(&phases, phase_len);
        let controller_arm_table = Self::build_controller_arm_table(&typed_typestate);
        let first_recv_dispatch = Self::build_first_recv_dispatch_table(&typed_typestate);
        unsafe {
            ptr::addr_of_mut!((*dst).role).write(ROLE);
            ProjectedRoleLayout::init(
                ptr::addr_of_mut!((*dst).layout),
                steps,
                len,
                phases,
                phase_len,
            );
            ptr::addr_of_mut!((*dst).typestate).write(typed_typestate.into_value());
            ptr::addr_of_mut!((*dst).eff_index_to_step).write(eff_index_to_step);
            ptr::addr_of_mut!((*dst).step_index_to_state).write(step_index_to_state);
            ptr::addr_of_mut!((*dst).active_lanes).write(active_lanes);
            ptr::addr_of_mut!((*dst).controller_arm_table).write(controller_arm_table);
            ptr::addr_of_mut!((*dst).first_recv_dispatch).write(first_recv_dispatch);
        }
    }

    #[inline(always)]
    pub(crate) const fn role(&self) -> u8 {
        self.role
    }

    #[inline(always)]
    #[cfg(test)]
    pub(crate) const fn layout(&self) -> &ProjectedRoleLayout {
        &self.layout
    }

    #[inline(always)]
    pub(crate) const fn phase_count(&self) -> usize {
        self.layout.phase_count()
    }

    #[inline(always)]
    pub(crate) const fn step_count(&self) -> usize {
        self.layout.len()
    }

    #[inline(always)]
    pub(crate) fn phase(&self, idx: usize) -> Option<&Phase> {
        if idx < self.layout.phase_count() {
            Some(&self.layout.phases()[idx])
        } else {
            None
        }
    }

    #[inline(always)]
    pub(crate) fn step(&self, idx: usize) -> Option<&LocalStep> {
        if idx < self.layout.len() {
            Some(&self.layout.steps()[idx])
        } else {
            None
        }
    }

    #[inline(always)]
    pub(crate) const fn typestate(&self) -> &RoleTypestateValue {
        &self.typestate
    }

    #[inline(always)]
    pub(crate) const fn step_for_eff_index(&self, idx: usize) -> Option<u16> {
        if idx < MAX_STEPS {
            Some(self.eff_index_to_step[idx])
        } else {
            None
        }
    }

    #[inline(always)]
    pub(crate) const fn state_for_step_index(&self, idx: usize) -> Option<StateIndex> {
        if idx < MAX_STEPS {
            Some(self.step_index_to_state[idx])
        } else {
            None
        }
    }

    #[inline(always)]
    pub(crate) const fn active_lanes(&self) -> &[bool; MAX_LANES] {
        &self.active_lanes
    }

    #[inline(always)]
    pub(crate) const fn controller_arm_entry_by_arm(
        &self,
        scope: ScopeId,
        arm: u8,
    ) -> Option<(StateIndex, u8)> {
        self.controller_arm_table.entry_by_arm(scope, arm)
    }

    #[inline(always)]
    pub(crate) const fn first_recv_dispatch_entry(
        &self,
        scope: ScopeId,
        idx: usize,
    ) -> Option<(u8, u8, StateIndex)> {
        self.first_recv_dispatch.entry(scope, idx)
    }

    fn build_active_lanes_from_phases(
        phases: &[Phase; MAX_PHASES],
        phase_len: usize,
    ) -> [bool; MAX_LANES] {
        let mut active = [false; MAX_LANES];
        let mut phase_idx = 0usize;
        while phase_idx < phase_len {
            let phase = phases[phase_idx];
            let mut lane_idx = 0usize;
            while lane_idx < MAX_LANES {
                if phase.lanes[lane_idx].is_active() {
                    active[lane_idx] = true;
                }
                lane_idx += 1;
            }
            phase_idx += 1;
        }
        active
    }

    const fn build_controller_arm_table<const ROLE: u8>(
        typestate: &RoleTypestate<ROLE>,
    ) -> ControllerArmTable {
        let mut table = ControllerArmTable::EMPTY;
        let mut ordinal = 0usize;
        while ordinal < crate::global::const_dsl::ScopeId::ORDINAL_CAPACITY as usize {
            let scope_id = ScopeId::route(ordinal as u16);
            let mut arm = 0u8;
            while arm <= 1 {
                if let Some((entry, label)) = typestate.controller_arm_entry_by_arm(scope_id, arm) {
                    if table.len >= ControllerArmTable::MAX_ENTRIES {
                        panic!("controller arm table capacity exceeded");
                    }
                    table.entries[table.len] = ControllerArmEntry {
                        scope: scope_id,
                        arm,
                        entry,
                        label,
                    };
                    table.len += 1;
                }
                if arm == 1 {
                    break;
                }
                arm += 1;
            }

            let loop_scope = ScopeId::loop_scope(ordinal as u16);
            let mut loop_arm = 0u8;
            while loop_arm <= 1 {
                if let Some((entry, label)) =
                    typestate.controller_arm_entry_by_arm(loop_scope, loop_arm)
                {
                    if table.len >= ControllerArmTable::MAX_ENTRIES {
                        panic!("controller arm table capacity exceeded");
                    }
                    table.entries[table.len] = ControllerArmEntry {
                        scope: loop_scope,
                        arm: loop_arm,
                        entry,
                        label,
                    };
                    table.len += 1;
                }
                if loop_arm == 1 {
                    break;
                }
                loop_arm += 1;
            }
            ordinal += 1;
        }
        table
    }

    const fn build_first_recv_dispatch_table<const ROLE: u8>(
        typestate: &RoleTypestate<ROLE>,
    ) -> FirstRecvDispatchTable {
        let mut table = FirstRecvDispatchTable::EMPTY;
        let mut ordinal = 0usize;
        while ordinal < crate::global::const_dsl::ScopeId::ORDINAL_CAPACITY as usize {
            let scope_id = ScopeId::route(ordinal as u16);
            let mut dispatch_idx = 0usize;
            loop {
                let Some((label, arm, target)) =
                    typestate.first_recv_dispatch_entry(scope_id, dispatch_idx)
                else {
                    break;
                };
                if table.len >= FirstRecvDispatchTable::MAX_ENTRIES {
                    panic!("first recv dispatch table capacity exceeded");
                }
                table.entries[table.len] = FirstRecvDispatchEntry {
                    scope: scope_id,
                    label,
                    arm,
                    target,
                };
                table.len += 1;
                dispatch_idx += 1;
            }
            ordinal += 1;
        }
        table
    }

    const fn build_local_steps<const ROLE: u8>(
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

    const fn build_step_index_to_state<const ROLE: u8>(
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
            panic!("eff_index out of bounds for compiled role mapping");
        }
        let step_idx = eff_index_to_step[eff_idx];
        if step_idx == MACHINE_NO_STEP {
            return;
        }
        let step_idx = step_idx as usize;
        if step_idx >= len {
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

    const fn build_phases<const ROLE: u8>(
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

    const fn build_route_guards_for_steps<const ROLE: u8>(
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

    #[inline(always)]
    const fn push_phase(phases: &mut [Phase; MAX_PHASES], phase_count: &mut usize, phase: Phase) {
        if *phase_count >= MAX_PHASES {
            panic!("compiled role phase capacity exceeded");
        }
        phases[*phase_count] = phase;
        *phase_count += 1;
    }

    const fn build_phases_with_parallel<const ROLE: u8>(
        steps: &[LocalStep; MAX_STEPS],
        len: usize,
        typestate: &RoleTypestate<ROLE>,
        route_guards: &[PhaseRouteGuard; MAX_STEPS],
    ) -> ([Phase; MAX_PHASES], usize) {
        let mut phases = [Phase::EMPTY; MAX_PHASES];
        let mut phase_count = 0usize;

        let mut parallel_ranges = [(0usize, 0usize); MAX_PHASES];
        let mut parallel_count = 0usize;
        loop {
            let Some(range) = typestate.parallel_phase_range_at(parallel_count) else {
                break;
            };
            if parallel_count >= MAX_PHASES {
                panic!("compiled role phase capacity exceeded");
            }
            parallel_ranges[parallel_count] = range;
            parallel_count += 1;
        }

        if parallel_count == 0 {
            return Self::build_single_phase(steps, len, route_guards);
        }

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
                Self::push_phase(
                    &mut phases,
                    &mut phase_count,
                    Self::build_phase_for_range(steps, seq_start, seq_end, route_guards),
                );
            }

            let par_start = seq_end;
            let mut par_end = par_start;
            while par_end < len && steps[par_end].eff_index().as_usize() < exit_eff {
                par_end += 1;
            }

            if par_end > par_start {
                Self::push_phase(
                    &mut phases,
                    &mut phase_count,
                    Self::build_phase_for_range(steps, par_start, par_end, route_guards),
                );
            }

            current_step = par_end;
            range_idx += 1;
        }

        if current_step < len {
            Self::push_phase(
                &mut phases,
                &mut phase_count,
                Self::build_phase_for_range(steps, current_step, len, route_guards),
            );
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
                let lane_start = lane_first[lane_idx];
                phase.lanes[lane_idx] = LaneSteps {
                    start: lane_start,
                    len: lane_lens[lane_idx],
                };
                lane_mask |= 1u8 << (lane_idx as u32);
                if lane_start < min_start {
                    min_start = lane_start;
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

#[cfg(test)]
mod tests {
    use crate::{
        control::{
            cap::mint::{
                CAP_HANDLE_LEN, CapError, CapShot, CapsMask, ControlMint, ControlResourceKind,
                GenericCapToken, MintConfig, ResourceKind, SessionScopedKind,
            },
            cap::resource_kinds::RouteDecisionKind,
            types::{Lane, SessionId},
        },
        g::{self, Msg, Role},
        global::{CanonicalControl, ControlHandling, role_program, typestate::PhaseCursor},
    };

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct RouteRightKind;

    impl ResourceKind for RouteRightKind {
        type Handle = (u8, u64);
        const TAG: u8 = 241;
        const NAME: &'static str = "RouteRightKind";
        const AUTO_MINT_EXTERNAL: bool = false;

        fn encode_handle(handle: &Self::Handle) -> [u8; CAP_HANDLE_LEN] {
            let mut out = [0u8; CAP_HANDLE_LEN];
            out[0] = handle.0;
            out[1..9].copy_from_slice(&handle.1.to_le_bytes());
            out
        }

        fn decode_handle(data: [u8; CAP_HANDLE_LEN]) -> Result<Self::Handle, CapError> {
            let mut raw = [0u8; 8];
            raw.copy_from_slice(&data[1..9]);
            Ok((data[0], u64::from_le_bytes(raw)))
        }

        fn zeroize(handle: &mut Self::Handle) {
            handle.0 = 0;
            handle.1 = 0;
        }

        fn caps_mask(_handle: &Self::Handle) -> CapsMask {
            CapsMask::empty()
        }

        fn scope_id(handle: &Self::Handle) -> Option<crate::global::const_dsl::ScopeId> {
            Some(crate::global::const_dsl::ScopeId::from_raw(handle.1))
        }
    }

    impl SessionScopedKind for RouteRightKind {
        fn handle_for_session(_sid: SessionId, _lane: Lane) -> Self::Handle {
            (0, crate::global::const_dsl::ScopeId::none().raw())
        }

        fn shot() -> CapShot {
            CapShot::One
        }
    }

    impl ControlResourceKind for RouteRightKind {
        const LABEL: u8 = 99;
        const SCOPE: crate::global::const_dsl::ControlScopeKind =
            crate::global::const_dsl::ControlScopeKind::Route;
        const TAP_ID: u16 = 0x03ff;
        const SHOT: CapShot = CapShot::One;
        const HANDLING: ControlHandling = ControlHandling::Canonical;
    }

    impl ControlMint for RouteRightKind {
        fn mint_handle(
            _sid: SessionId,
            _lane: Lane,
            scope: crate::global::const_dsl::ScopeId,
        ) -> Self::Handle {
            (1, scope.raw())
        }
    }

    #[test]
    fn compiled_role_exposes_controller_arm_and_dispatch_tables() {
        let left = g::seq(
            g::send::<
                Role<0>,
                Role<0>,
                Msg<
                    { crate::runtime::consts::LABEL_ROUTE_DECISION },
                    GenericCapToken<RouteDecisionKind>,
                    CanonicalControl<RouteDecisionKind>,
                >,
                0,
            >(),
            g::send::<Role<0>, Role<1>, Msg<41, ()>, 0>(),
        );
        let right = g::seq(
            g::send::<
                Role<0>,
                Role<0>,
                Msg<99, GenericCapToken<RouteRightKind>, CanonicalControl<RouteRightKind>>,
                0,
            >(),
            g::send::<Role<0>, Role<1>, Msg<47, ()>, 0>(),
        );
        let program = g::route(left, right);

        let controller: crate::g::advanced::RoleProgram<'_, 0, _, MintConfig> =
            role_program::project(&program);
        let controller_compiled = controller.compile_role();
        let controller_scope = PhaseCursor::new(&controller_compiled).node_scope_id();
        assert_eq!(controller_compiled.role(), 0);
        assert!(controller_compiled.active_lanes()[0]);
        assert_eq!(
            controller_compiled
                .controller_arm_entry_by_arm(controller_scope, 0)
                .map(|(_, label)| label),
            Some(crate::runtime::consts::LABEL_ROUTE_DECISION)
        );
        assert_eq!(
            controller_compiled
                .controller_arm_entry_by_arm(controller_scope, 1)
                .map(|(_, label)| label),
            Some(99)
        );

        let worker: crate::g::advanced::RoleProgram<'_, 1, _, MintConfig> =
            role_program::project(&program);
        let worker_compiled = worker.compile_role();
        let worker_scope = PhaseCursor::new(&worker_compiled).node_scope_id();
        assert_eq!(worker_compiled.role(), 1);
        assert!(worker_compiled.active_lanes()[0]);
        assert!(worker_compiled.phase_count() > 0);
        assert!(worker_compiled.step_count() > 0);
        assert_eq!(
            worker_compiled
                .first_recv_dispatch_entry(worker_scope, 0)
                .map(|(label, arm, _)| (label, arm)),
            Some((41, 0))
        );
        assert_eq!(
            worker_compiled
                .first_recv_dispatch_entry(worker_scope, 1)
                .map(|(label, arm, _)| (label, arm)),
            Some((47, 1))
        );
        assert!(worker_compiled.step_for_eff_index(0).is_some());
        assert!(worker_compiled.state_for_step_index(0).is_some());
    }
}
