use core::ptr;

use crate::endpoint::kernel::{EndpointArenaLayout, FrontierScratchLayout};
#[cfg(test)]
use crate::global::role_program::ProjectedRoleLayout;
use crate::global::role_program::{
    LaneSteps, LocalStep, MAX_LANES, MAX_PHASES, MAX_STEPS, Phase, PhaseRouteGuard,
};
#[cfg(test)]
use crate::global::typestate::RoleTypestate;
use crate::global::typestate::{
    LocalAction, LocalNode, MAX_STATES, RoleCompileScratch, RoleTypestateValue, RouteScopeRecord,
    ScopeRecord, StateIndex,
};

use super::LoweringSummary;

const MACHINE_NO_STEP: u16 = u16::MAX;
const RESERVED_BINDING_LANES: usize = 2;
#[cfg(test)]
const TEST_FRONTIER_ENTRY_FLOOR: usize = 8;

#[inline(always)]
const fn test_frontier_entry_capacity(compiled: usize) -> usize {
    #[cfg(test)]
    {
        if compiled > TEST_FRONTIER_ENTRY_FLOOR {
            compiled
        } else {
            TEST_FRONTIER_ENTRY_FLOOR
        }
    }
    #[cfg(not(test))]
    {
        compiled
    }
}

#[inline(always)]
const fn encode_compact_step_index(value: usize) -> u16 {
    if value > u16::MAX as usize {
        panic!("compiled role compact index overflow");
    }
    value as u16
}

#[inline(always)]
const fn encode_compact_count_u16(value: usize) -> u16 {
    if value > u16::MAX as usize {
        panic!("compiled role compact count overflow");
    }
    value as u16
}

#[inline(always)]
const fn encode_compact_count_u8(value: usize) -> u8 {
    if value > u8::MAX as usize {
        panic!("compiled role compact count overflow");
    }
    value as u8
}

/// Crate-private owner for lowered role-local facts.
#[cfg(test)]
#[derive(Clone, Debug)]
pub(crate) struct CompiledRole {
    role: u8,
    layout: ProjectedRoleLayout,
    typestate: RoleTypestate<0>,
    scope_records: [crate::global::typestate::ScopeRecord; crate::eff::meta::MAX_EFF_NODES],
    scope_slots_by_scope: [u16; crate::eff::meta::MAX_EFF_NODES],
    scope_route_dense_by_slot: [u16; crate::eff::meta::MAX_EFF_NODES],
    route_scope_records:
        [crate::global::typestate::RouteScopeRecord; crate::eff::meta::MAX_EFF_NODES],
    eff_index_to_step: [u16; MAX_STEPS],
    step_index_to_state: [StateIndex; MAX_STEPS],
}

/// Crate-private runtime image for role-local immutable facts.
#[derive(Clone, Debug)]
pub(crate) struct CompiledRoleImage {
    phases: *const Phase,
    typestate: *const RoleTypestateValue,
    eff_index_to_step: *const u16,
    step_index_to_state: *const StateIndex,
    role: u8,
    active_lane_mask_bits: u8,
    active_lane_count_value: u8,
    logical_lane_count_value: u8,
    endpoint_lane_slot_count_value: u8,
    compiled_frontier_entries: u8,
    phase_len: u16,
    route_scope_count_value: u16,
    max_route_stack_depth_value: u16,
    max_loop_stack_depth_value: u16,
    eff_index_to_step_len: u16,
    step_index_to_state_len: u16,
}

struct CompiledRoleScopeStorage {
    typestate: *mut RoleTypestateValue,
    typestate_nodes: *mut LocalNode,
    typestate_node_cap: usize,
    phases: *mut Phase,
    phase_cap: usize,
    records: *mut ScopeRecord,
    slots_by_scope: *mut u16,
    route_dense_by_slot: *mut u16,
    route_records: *mut RouteScopeRecord,
    route_scope_cap: usize,
    scope_cap: usize,
    eff_index_to_step: *mut u16,
    step_index_to_state: *mut StateIndex,
}

impl CompiledRoleScopeStorage {
    #[inline(always)]
    const fn align_up(value: usize, align: usize) -> usize {
        let mask = align.saturating_sub(1);
        (value + mask) & !mask
    }

    #[inline(always)]
    const fn scope_cap(scope_count: usize) -> usize {
        scope_count
    }

    #[inline(always)]
    const fn route_scope_cap(route_scope_count: usize) -> usize {
        route_scope_count
    }

    #[inline(always)]
    const fn step_cap(eff_count: usize) -> usize {
        if eff_count == 0 { 1 } else { eff_count }
    }

    #[inline(always)]
    const fn typestate_node_cap(
        scope_count: usize,
        passive_linger_route_scope_count: usize,
        local_step_count: usize,
    ) -> usize {
        // Local nodes cover the projected local steps plus at most one boundary
        // node per structured scope. Passive linger route scopes may additionally need
        // one arm-navigation jump beyond that base budget, plus one terminal slot.
        let capped = local_step_count
            .saturating_add(scope_count)
            .saturating_add(passive_linger_route_scope_count)
            .saturating_add(1);
        if capped == 0 {
            1
        } else if capped < MAX_STATES {
            capped
        } else {
            MAX_STATES
        }
    }

    #[inline(always)]
    const fn phase_cap(local_step_count: usize, parallel_enter_count: usize) -> usize {
        if local_step_count == 0 {
            1
        } else {
            let derived = parallel_enter_count.saturating_mul(2).saturating_add(1);
            let capped = if derived < local_step_count {
                derived
            } else {
                local_step_count
            };
            if capped == 0 {
                1
            } else if capped < MAX_PHASES {
                capped
            } else {
                MAX_PHASES
            }
        }
    }

    #[inline(always)]
    const fn total_bytes_for_layout(
        scope_count: usize,
        passive_linger_route_scope_count: usize,
        route_scope_count: usize,
        parallel_enter_count: usize,
        eff_count: usize,
        step_index_to_state_count: usize,
    ) -> usize {
        let scope_cap = Self::scope_cap(scope_count);
        let route_scope_cap = Self::route_scope_cap(route_scope_count);
        let eff_index_cap = Self::step_cap(eff_count);
        let step_index_cap = Self::step_cap(step_index_to_state_count);
        let typestate_node_cap = Self::typestate_node_cap(
            scope_count,
            passive_linger_route_scope_count,
            step_index_to_state_count,
        );
        let phase_cap = Self::phase_cap(step_index_to_state_count, parallel_enter_count);
        let header = core::mem::size_of::<CompiledRoleImage>();
        let typestate_start = Self::align_up(
            header,
            if core::mem::align_of::<RoleTypestateValue>()
                > core::mem::align_of::<CompiledRoleImage>()
            {
                core::mem::align_of::<RoleTypestateValue>()
            } else {
                core::mem::align_of::<CompiledRoleImage>()
            },
        );
        let typestate_end = typestate_start + core::mem::size_of::<RoleTypestateValue>();
        let typestate_nodes_start =
            Self::align_up(typestate_end, core::mem::align_of::<LocalNode>());
        let typestate_nodes_end = typestate_nodes_start
            + typestate_node_cap.saturating_mul(core::mem::size_of::<LocalNode>());
        let phases_start = Self::align_up(typestate_nodes_end, core::mem::align_of::<Phase>());
        let phases_end = phases_start + phase_cap.saturating_mul(core::mem::size_of::<Phase>());
        let records_start = Self::align_up(phases_end, core::mem::align_of::<ScopeRecord>());
        let records_end =
            records_start + scope_cap.saturating_mul(core::mem::size_of::<ScopeRecord>());
        let slots_start = Self::align_up(records_end, core::mem::align_of::<u16>());
        let slots_end = slots_start + scope_cap.saturating_mul(core::mem::size_of::<u16>());
        let route_dense_start = Self::align_up(slots_end, core::mem::align_of::<u16>());
        let route_dense_end =
            route_dense_start + scope_cap.saturating_mul(core::mem::size_of::<u16>());
        let route_records_start =
            Self::align_up(route_dense_end, core::mem::align_of::<RouteScopeRecord>());
        let route_records_end = route_records_start
            + route_scope_cap.saturating_mul(core::mem::size_of::<RouteScopeRecord>());
        let eff_index_start = Self::align_up(route_records_end, core::mem::align_of::<u16>());
        let eff_index_end =
            eff_index_start + eff_index_cap.saturating_mul(core::mem::size_of::<u16>());
        let step_index_start = Self::align_up(eff_index_end, core::mem::align_of::<StateIndex>());
        step_index_start + step_index_cap.saturating_mul(core::mem::size_of::<StateIndex>())
    }

    #[cfg(test)]
    #[inline(always)]
    const fn total_bytes_for_counts(
        scope_count: usize,
        route_scope_count: usize,
        eff_count: usize,
    ) -> usize {
        Self::total_bytes_for_layout(
            scope_count,
            route_scope_count,
            route_scope_count,
            scope_count,
            eff_count,
            eff_count,
        )
    }

    #[inline(always)]
    const fn overall_align() -> usize {
        let mut align = core::mem::align_of::<CompiledRoleImage>();
        if core::mem::align_of::<RoleTypestateValue>() > align {
            align = core::mem::align_of::<RoleTypestateValue>();
        }
        if core::mem::align_of::<LocalNode>() > align {
            align = core::mem::align_of::<LocalNode>();
        }
        if core::mem::align_of::<Phase>() > align {
            align = core::mem::align_of::<Phase>();
        }
        if core::mem::align_of::<ScopeRecord>() > align {
            align = core::mem::align_of::<ScopeRecord>();
        }
        if core::mem::align_of::<RouteScopeRecord>() > align {
            align = core::mem::align_of::<RouteScopeRecord>();
        }
        if core::mem::align_of::<StateIndex>() > align {
            align = core::mem::align_of::<StateIndex>();
        }
        align
    }

    #[inline(always)]
    unsafe fn from_image_ptr_with_layout(
        image: *mut CompiledRoleImage,
        scope_count: usize,
        passive_linger_route_scope_count: usize,
        route_scope_count: usize,
        parallel_enter_count: usize,
        eff_count: usize,
        step_index_to_state_count: usize,
    ) -> Self {
        let scope_cap = Self::scope_cap(scope_count);
        let route_scope_cap = Self::route_scope_cap(route_scope_count);
        let eff_index_cap = Self::step_cap(eff_count);
        let typestate_node_cap = Self::typestate_node_cap(
            scope_count,
            passive_linger_route_scope_count,
            step_index_to_state_count,
        );
        let phase_cap = Self::phase_cap(step_index_to_state_count, parallel_enter_count);
        let base = image.cast::<u8>() as usize;
        let header_end = base + core::mem::size_of::<CompiledRoleImage>();
        let typestate_start =
            Self::align_up(header_end, core::mem::align_of::<RoleTypestateValue>());
        let typestate_end = typestate_start + core::mem::size_of::<RoleTypestateValue>();
        let typestate_nodes_start =
            Self::align_up(typestate_end, core::mem::align_of::<LocalNode>());
        let typestate_nodes_end = typestate_nodes_start
            + typestate_node_cap.saturating_mul(core::mem::size_of::<LocalNode>());
        let phases_start = Self::align_up(typestate_nodes_end, core::mem::align_of::<Phase>());
        let phases_end = phases_start + phase_cap.saturating_mul(core::mem::size_of::<Phase>());
        let records_start = Self::align_up(phases_end, core::mem::align_of::<ScopeRecord>());
        let records_end =
            records_start + scope_cap.saturating_mul(core::mem::size_of::<ScopeRecord>());
        let slots_start = Self::align_up(records_end, core::mem::align_of::<u16>());
        let slots_end = slots_start + scope_cap.saturating_mul(core::mem::size_of::<u16>());
        let route_dense_start = Self::align_up(slots_end, core::mem::align_of::<u16>());
        let route_dense_end =
            route_dense_start + scope_cap.saturating_mul(core::mem::size_of::<u16>());
        let route_records_start =
            Self::align_up(route_dense_end, core::mem::align_of::<RouteScopeRecord>());
        let route_records_end = route_records_start
            + route_scope_cap.saturating_mul(core::mem::size_of::<RouteScopeRecord>());
        let eff_index_start = Self::align_up(route_records_end, core::mem::align_of::<u16>());
        let eff_index_end =
            eff_index_start + eff_index_cap.saturating_mul(core::mem::size_of::<u16>());
        let step_index_start = Self::align_up(eff_index_end, core::mem::align_of::<StateIndex>());
        Self {
            typestate: typestate_start as *mut RoleTypestateValue,
            typestate_nodes: typestate_nodes_start as *mut LocalNode,
            typestate_node_cap,
            phases: phases_start as *mut Phase,
            phase_cap,
            records: records_start as *mut ScopeRecord,
            slots_by_scope: slots_start as *mut u16,
            route_dense_by_slot: route_dense_start as *mut u16,
            route_records: route_records_start as *mut RouteScopeRecord,
            route_scope_cap,
            scope_cap,
            eff_index_to_step: eff_index_start as *mut u16,
            step_index_to_state: step_index_start as *mut StateIndex,
        }
    }
}

#[cfg(test)]
impl CompiledRole {
    #[inline(never)]
    pub(crate) unsafe fn init_from_summary<const ROLE: u8>(
        dst: *mut Self,
        summary: &LoweringSummary,
        scratch: &mut RoleCompileScratch,
    ) {
        unsafe {
            ptr::addr_of_mut!((*dst).role).write(ROLE);
            ptr::addr_of_mut!((*dst).scope_records).write(
                [crate::global::typestate::ScopeRecord::EMPTY; crate::eff::meta::MAX_EFF_NODES],
            );
            ptr::addr_of_mut!((*dst).scope_slots_by_scope)
                .write([u16::MAX; crate::eff::meta::MAX_EFF_NODES]);
            ptr::addr_of_mut!((*dst).scope_route_dense_by_slot)
                .write([u16::MAX; crate::eff::meta::MAX_EFF_NODES]);
            ptr::addr_of_mut!((*dst).route_scope_records).write(
                [crate::global::typestate::RouteScopeRecord::EMPTY;
                    crate::eff::meta::MAX_EFF_NODES],
            );
            RoleTypestate::<ROLE>::init_value_from_summary(
                ptr::addr_of_mut!((*dst).typestate).cast::<RoleTypestate<ROLE>>(),
                &mut *ptr::addr_of_mut!((*dst).scope_records),
                ptr::addr_of_mut!((*dst).scope_slots_by_scope).cast::<u16>(),
                ptr::addr_of_mut!((*dst).scope_route_dense_by_slot).cast::<u16>(),
                ptr::addr_of_mut!((*dst).route_scope_records)
                    .cast::<crate::global::typestate::RouteScopeRecord>(),
                crate::eff::meta::MAX_EFF_NODES,
                summary,
                scratch,
            );
        }
        let typed_typestate =
            unsafe { &*core::ptr::addr_of!((*dst).typestate).cast::<RoleTypestate<ROLE>>() };
        let len = Self::build_local_steps_into::<ROLE>(
            typed_typestate,
            &mut scratch.by_eff_index,
            &mut scratch.present,
            &mut scratch.steps,
            &mut scratch.eff_index_to_step,
        );
        Self::build_step_index_to_state_into::<ROLE>(
            typed_typestate,
            &scratch.steps,
            len,
            &scratch.eff_index_to_step,
            &mut scratch.step_index_to_state,
        );
        let phase_len = Self::build_phases_into::<ROLE>(
            &scratch.steps,
            len,
            typed_typestate,
            &scratch.step_index_to_state,
            &mut scratch.route_guards,
            &mut scratch.phases,
            &mut scratch.parallel_ranges,
        );
        unsafe {
            ProjectedRoleLayout::init_from_refs(
                ptr::addr_of_mut!((*dst).layout),
                &scratch.steps,
                len,
                &scratch.phases,
                phase_len,
            );
            core::ptr::copy_nonoverlapping(
                scratch.eff_index_to_step.as_ptr(),
                ptr::addr_of_mut!((*dst).eff_index_to_step).cast::<u16>(),
                MAX_STEPS,
            );
            core::ptr::copy_nonoverlapping(
                scratch.step_index_to_state.as_ptr(),
                ptr::addr_of_mut!((*dst).step_index_to_state).cast::<StateIndex>(),
                MAX_STEPS,
            );
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

    #[cfg(test)]
    #[inline(always)]
    pub(crate) fn phase_count(&self) -> usize {
        self.layout.phase_count()
    }

    #[cfg(test)]
    #[inline(always)]
    pub(crate) fn step_count(&self) -> usize {
        self.layout.len()
    }

    #[cfg(test)]
    #[inline(always)]
    pub(crate) const fn typestate(&self) -> &RoleTypestate<0> {
        &self.typestate
    }

    #[inline(always)]
    pub(crate) const fn typestate_ref(&self) -> &RoleTypestate<0> {
        &self.typestate
    }

    #[cfg(test)]
    #[inline(always)]
    pub(crate) const fn step_for_eff_index(&self, idx: usize) -> Option<u16> {
        if idx < MAX_STEPS {
            Some(self.eff_index_to_step[idx])
        } else {
            None
        }
    }

    #[cfg(test)]
    #[inline(always)]
    pub(crate) const fn state_for_step_index(&self, idx: usize) -> Option<StateIndex> {
        if idx < MAX_STEPS {
            Some(self.step_index_to_state[idx])
        } else {
            None
        }
    }

    #[inline(always)]
    pub(crate) fn is_active_lane(&self, lane_idx: usize) -> bool {
        lane_idx < MAX_LANES && ((self.active_lane_mask() >> lane_idx) & 1) != 0
    }

    #[inline(always)]
    fn active_lane_mask(&self) -> u8 {
        build_active_lane_mask_from_phase_slice(self.layout.phases())
    }

    #[cfg(test)]
    #[inline(always)]
    pub(crate) fn controller_arm_entry_by_arm(
        &self,
        scope: crate::global::const_dsl::ScopeId,
        arm: u8,
    ) -> Option<(StateIndex, u8)> {
        self.typestate.controller_arm_entry_by_arm(scope, arm)
    }

    #[cfg(test)]
    #[inline(always)]
    pub(crate) fn first_recv_dispatch_entry(
        &self,
        scope: crate::global::const_dsl::ScopeId,
        idx: usize,
    ) -> Option<(u8, u8, StateIndex)> {
        self.typestate.first_recv_dispatch_entry(scope, idx)
    }

    fn build_local_steps_into<const ROLE: u8>(
        typestate: &RoleTypestate<ROLE>,
        by_eff_index: &mut [LocalStep; MAX_STEPS],
        present: &mut [bool; MAX_STEPS],
        steps: &mut [LocalStep; MAX_STEPS],
        eff_index_to_step: &mut [u16; MAX_STEPS],
    ) -> usize {
        let mut idx = 0usize;
        while idx < MAX_STEPS {
            by_eff_index[idx] = LocalStep::EMPTY;
            present[idx] = false;
            steps[idx] = LocalStep::EMPTY;
            eff_index_to_step[idx] = MACHINE_NO_STEP;
            idx += 1;
        }

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
                LocalAction::Terminate | LocalAction::Jump { .. } => {}
            }
            node_idx += 1;
        }

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
        len
    }

    fn build_step_index_to_state_into<const ROLE: u8>(
        typestate: &RoleTypestate<ROLE>,
        steps: &[LocalStep; MAX_STEPS],
        len: usize,
        eff_index_to_step: &[u16; MAX_STEPS],
        step_index_to_state: &mut [StateIndex; MAX_STEPS],
    ) {
        let mut idx = 0usize;
        while idx < MAX_STEPS {
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
                } => Self::record_step_state(
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
                } => Self::record_step_state(
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
                } => Self::record_step_state(
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

    fn build_phases_into<const ROLE: u8>(
        steps: &[LocalStep; MAX_STEPS],
        len: usize,
        typestate: &RoleTypestate<ROLE>,
        step_index_to_state: &[StateIndex; MAX_STEPS],
        route_guards: &mut [PhaseRouteGuard; MAX_STEPS],
        phases: &mut [Phase; MAX_PHASES],
        parallel_ranges: &mut [(usize, usize); MAX_PHASES],
    ) -> usize {
        let mut phase_idx = 0usize;
        while phase_idx < MAX_PHASES {
            phases[phase_idx] = Phase::EMPTY;
            parallel_ranges[phase_idx] = (0, 0);
            phase_idx += 1;
        }
        if len == 0 {
            return 0;
        }

        Self::build_route_guards_for_steps_into(len, typestate, step_index_to_state, route_guards);

        if !typestate.has_parallel_phase_scope() {
            phases[0] = build_phase_for_range(steps, 0, len, route_guards);
            return 1;
        }

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
            phases[0] = build_phase_for_range(steps, 0, len, route_guards);
            return 1;
        }

        let mut phase_count = 0usize;
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
                push_phase(
                    phases,
                    &mut phase_count,
                    build_phase_for_range(steps, seq_start, seq_end, route_guards),
                );
            }

            let par_start = seq_end;
            let mut par_end = par_start;
            while par_end < len && steps[par_end].eff_index().as_usize() < exit_eff {
                par_end += 1;
            }
            if par_end > par_start {
                push_phase(
                    phases,
                    &mut phase_count,
                    build_phase_for_range(steps, par_start, par_end, route_guards),
                );
            }

            current_step = par_end;
            range_idx += 1;
        }

        if current_step < len {
            push_phase(
                phases,
                &mut phase_count,
                build_phase_for_range(steps, current_step, len, route_guards),
            );
        }

        if phase_count == 0 {
            phases[0] = build_phase_for_range(steps, 0, len, route_guards);
            return 1;
        }
        phase_count
    }

    fn build_route_guards_for_steps_into<const ROLE: u8>(
        len: usize,
        typestate: &RoleTypestate<ROLE>,
        step_index_to_state: &[StateIndex; MAX_STEPS],
        route_guards: &mut [PhaseRouteGuard; MAX_STEPS],
    ) {
        let mut idx = 0usize;
        while idx < MAX_STEPS {
            route_guards[idx] = PhaseRouteGuard::EMPTY;
            idx += 1;
        }
        let mut step_idx = 0usize;
        while step_idx < len {
            let state = step_index_to_state[step_idx];
            if let Some((scope, arm)) =
                crate::global::typestate::phase_route_guard_for_built_state_for_role(
                    typestate, ROLE, state,
                )
            {
                route_guards[step_idx] = PhaseRouteGuard::new(scope, arm);
            }
            step_idx += 1;
        }
    }
}

fn build_active_lane_mask_from_phase_slice(phases: &[Phase]) -> u8 {
    let mut mask = 0u8;
    let mut phase_idx = 0usize;
    while phase_idx < phases.len() {
        let phase = phases[phase_idx];
        let mut lane_idx = 0usize;
        while lane_idx < MAX_LANES {
            if phase.lanes[lane_idx].is_active() {
                mask |= 1u8 << lane_idx;
            }
            lane_idx += 1;
        }
        phase_idx += 1;
    }
    mask
}

#[inline(always)]
fn active_lane_count_from_mask(mask: u8) -> usize {
    let clipped = if MAX_LANES >= u8::BITS as usize {
        mask
    } else {
        mask & ((1u8 << MAX_LANES) - 1)
    };
    clipped.count_ones() as usize
}

fn build_local_steps_into(
    role: u8,
    typestate: &RoleTypestateValue,
    by_eff_index: &mut [LocalStep; MAX_STEPS],
    present: &mut [bool; MAX_STEPS],
    steps: &mut [LocalStep; MAX_STEPS],
    eff_index_to_step: &mut [u16; MAX_STEPS],
) -> usize {
    let mut idx = 0usize;
    while idx < MAX_STEPS {
        by_eff_index[idx] = LocalStep::EMPTY;
        present[idx] = false;
        steps[idx] = LocalStep::EMPTY;
        eff_index_to_step[idx] = MACHINE_NO_STEP;
        idx += 1;
    }

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
                    by_eff_index[idx] =
                        LocalStep::send(eff_index, peer, label, resource, is_control, shot, lane);
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
                    by_eff_index[idx] =
                        LocalStep::recv(eff_index, peer, label, resource, is_control, shot, lane);
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
                    by_eff_index[idx] =
                        LocalStep::local(eff_index, role, label, resource, is_control, shot, lane);
                    present[idx] = true;
                }
            }
            LocalAction::Terminate | LocalAction::Jump { .. } => {}
        }
        node_idx += 1;
    }

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
    len
}

fn build_step_index_to_state_into(
    typestate: &RoleTypestateValue,
    steps: &[LocalStep; MAX_STEPS],
    len: usize,
    eff_index_to_step: &[u16; MAX_STEPS],
    step_index_to_state: &mut [StateIndex; MAX_STEPS],
) {
    let mut idx = 0usize;
    while idx < MAX_STEPS {
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

fn build_phases_into(
    role: u8,
    steps: &[LocalStep; MAX_STEPS],
    len: usize,
    typestate: &RoleTypestateValue,
    step_index_to_state: &[StateIndex; MAX_STEPS],
    route_guards: &mut [PhaseRouteGuard; MAX_STEPS],
    phases: &mut [Phase; MAX_PHASES],
    parallel_ranges: &mut [(usize, usize); MAX_PHASES],
) -> usize {
    let mut phase_idx = 0usize;
    while phase_idx < MAX_PHASES {
        phases[phase_idx] = Phase::EMPTY;
        parallel_ranges[phase_idx] = (0, 0);
        phase_idx += 1;
    }
    if len == 0 {
        return 0;
    }

    build_route_guards_for_steps_into(role, len, typestate, step_index_to_state, route_guards);

    if !typestate.has_parallel_phase_scope() {
        phases[0] = build_phase_for_range(steps, 0, len, route_guards);
        return 1;
    }

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
        phases[0] = build_phase_for_range(steps, 0, len, route_guards);
        return 1;
    }

    let mut phase_count = 0usize;
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
            push_phase(
                phases,
                &mut phase_count,
                build_phase_for_range(steps, seq_start, seq_end, route_guards),
            );
        }

        let par_start = seq_end;
        let mut par_end = par_start;
        while par_end < len && steps[par_end].eff_index().as_usize() < exit_eff {
            par_end += 1;
        }
        if par_end > par_start {
            push_phase(
                phases,
                &mut phase_count,
                build_phase_for_range(steps, par_start, par_end, route_guards),
            );
        }

        current_step = par_end;
        range_idx += 1;
    }

    if current_step < len {
        push_phase(
            phases,
            &mut phase_count,
            build_phase_for_range(steps, current_step, len, route_guards),
        );
    }

    if phase_count == 0 {
        phases[0] = build_phase_for_range(steps, 0, len, route_guards);
        return 1;
    }
    phase_count
}

fn build_route_guards_for_steps_into(
    role: u8,
    len: usize,
    typestate: &RoleTypestateValue,
    step_index_to_state: &[StateIndex; MAX_STEPS],
    route_guards: &mut [PhaseRouteGuard; MAX_STEPS],
) {
    let mut idx = 0usize;
    while idx < MAX_STEPS {
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

#[inline(always)]
const fn push_phase(phases: &mut [Phase; MAX_PHASES], phase_count: &mut usize, phase: Phase) {
    if *phase_count >= MAX_PHASES {
        panic!("compiled role phase capacity exceeded");
    }
    phases[*phase_count] = phase;
    *phase_count += 1;
}

const fn build_phase_for_range(
    steps: &[LocalStep; MAX_STEPS],
    start: usize,
    end: usize,
    route_guards: &[PhaseRouteGuard; MAX_STEPS],
) -> Phase {
    let mut phase = Phase::EMPTY;
    let mut lane_lens = [0u16; MAX_LANES];
    let mut lane_first = [u16::MAX; MAX_LANES];

    let mut i = start;
    while i < end {
        let lane = steps[i].lane() as usize;
        if lane < MAX_LANES {
            if lane_first[lane] == u16::MAX {
                lane_first[lane] = encode_compact_step_index(i);
            }
            if lane_lens[lane] == u16::MAX {
                panic!("phase lane length overflow");
            }
            lane_lens[lane] += 1;
        }
        i += 1;
    }

    let mut lane_mask = 0u8;
    let mut min_start = u16::MAX;
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
    phase.route_guard = route_guard_for_range(route_guards, start, end);
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

#[inline(never)]
unsafe fn init_empty_compiled_role_image(dst: *mut CompiledRoleImage, role: u8) {
    unsafe { CompiledRoleImage::init_empty_compiled_role(dst, role) };
}

#[inline(never)]
unsafe fn finalize_compiled_role_image_from_typestate(
    dst: *mut CompiledRoleImage,
    scratch: &mut RoleCompileScratch,
) {
    unsafe { CompiledRoleImage::finalize_compiled_role_from_typestate(dst, scratch) };
}

impl CompiledRoleImage {
    #[cfg(test)]
    #[inline(always)]
    pub(crate) const fn persistent_bytes_for_counts(
        scope_count: usize,
        route_scope_count: usize,
        eff_count: usize,
    ) -> usize {
        CompiledRoleScopeStorage::total_bytes_for_counts(scope_count, route_scope_count, eff_count)
    }

    #[inline(always)]
    pub(crate) const fn persistent_bytes_for_program(
        scope_count: usize,
        passive_linger_route_scope_count: usize,
        route_scope_count: usize,
        parallel_enter_count: usize,
        eff_count: usize,
        local_step_count: usize,
    ) -> usize {
        CompiledRoleScopeStorage::total_bytes_for_layout(
            scope_count,
            passive_linger_route_scope_count,
            route_scope_count,
            parallel_enter_count,
            eff_count,
            local_step_count,
        )
    }

    #[inline(always)]
    pub(crate) const fn persistent_align() -> usize {
        CompiledRoleScopeStorage::overall_align()
    }

    #[inline(never)]
    unsafe fn init_empty_compiled_role(dst: *mut Self, role: u8) {
        unsafe {
            ptr::addr_of_mut!((*dst).typestate).write(core::ptr::null());
            ptr::addr_of_mut!((*dst).eff_index_to_step).write(core::ptr::null());
            ptr::addr_of_mut!((*dst).phases).write(core::ptr::null());
            ptr::addr_of_mut!((*dst).step_index_to_state).write(core::ptr::null());
            ptr::addr_of_mut!((*dst).role).write(role);
            ptr::addr_of_mut!((*dst).active_lane_mask_bits).write(0);
            ptr::addr_of_mut!((*dst).active_lane_count_value).write(0);
            ptr::addr_of_mut!((*dst).logical_lane_count_value).write(0);
            ptr::addr_of_mut!((*dst).endpoint_lane_slot_count_value).write(0);
            ptr::addr_of_mut!((*dst).compiled_frontier_entries).write(0);
            ptr::addr_of_mut!((*dst).phase_len).write(0);
            ptr::addr_of_mut!((*dst).route_scope_count_value).write(0);
            ptr::addr_of_mut!((*dst).max_route_stack_depth_value).write(0);
            ptr::addr_of_mut!((*dst).max_loop_stack_depth_value).write(0);
            ptr::addr_of_mut!((*dst).eff_index_to_step_len).write(0);
            ptr::addr_of_mut!((*dst).step_index_to_state_len).write(0);
        }
    }

    #[inline(never)]
    unsafe fn finalize_compiled_role_from_typestate(
        dst: *mut Self,
        scratch: &mut RoleCompileScratch,
    ) {
        let role = unsafe { (*dst).role };
        let typed_typestate = unsafe { &*(*dst).typestate };
        let len = build_local_steps_into(
            role,
            typed_typestate,
            &mut scratch.by_eff_index,
            &mut scratch.present,
            &mut scratch.steps,
            &mut scratch.eff_index_to_step,
        );
        build_step_index_to_state_into(
            typed_typestate,
            &scratch.steps,
            len,
            &scratch.eff_index_to_step,
            &mut scratch.step_index_to_state,
        );
        let step_state_cap = unsafe { (*dst).step_index_to_state_len as usize };
        if len > step_state_cap {
            panic!("compiled role local step count exceeds allocated step-state capacity");
        }
        unsafe {
            ptr::addr_of_mut!((*dst).step_index_to_state_len).write(encode_compact_count_u16(len));
        }
        let phase_len = build_phases_into(
            role,
            &scratch.steps,
            len,
            typed_typestate,
            &scratch.step_index_to_state,
            &mut scratch.route_guards,
            &mut scratch.phases,
            &mut scratch.parallel_ranges,
        );
        let phase_cap = unsafe { (*dst).phase_len as usize };
        if phase_len > phase_cap {
            panic!("compiled role phase count exceeds allocated phase capacity");
        }
        let active_lane_mask =
            build_active_lane_mask_from_phase_slice(&scratch.phases[..phase_len]);
        let active_lane_count = active_lane_count_from_mask(active_lane_mask);
        let logical_lane_count = Self::binding_lane_count(active_lane_count);
        let endpoint_lane_slot_count = Self::endpoint_lane_slot_count_from_mask(active_lane_mask);
        let route_scope_count = typed_typestate.route_scope_count();
        let max_route_stack_depth = typed_typestate.max_route_stack_depth();
        let max_loop_stack_depth = typed_typestate.max_loop_stack_depth();
        let compiled_frontier_entries = core::cmp::max(
            core::cmp::min(typed_typestate.max_offer_entries(), MAX_LANES),
            usize::from(route_scope_count != 0),
        );
        unsafe {
            ptr::addr_of_mut!((*dst).active_lane_mask_bits).write(active_lane_mask);
            ptr::addr_of_mut!((*dst).active_lane_count_value)
                .write(encode_compact_count_u8(active_lane_count));
            ptr::addr_of_mut!((*dst).logical_lane_count_value)
                .write(encode_compact_count_u8(logical_lane_count));
            ptr::addr_of_mut!((*dst).endpoint_lane_slot_count_value)
                .write(encode_compact_count_u8(endpoint_lane_slot_count));
            ptr::addr_of_mut!((*dst).compiled_frontier_entries)
                .write(encode_compact_count_u8(compiled_frontier_entries));
            ptr::addr_of_mut!((*dst).phase_len).write(encode_compact_count_u16(phase_len));
            ptr::addr_of_mut!((*dst).route_scope_count_value)
                .write(encode_compact_count_u16(route_scope_count));
            ptr::addr_of_mut!((*dst).max_route_stack_depth_value)
                .write(encode_compact_count_u16(max_route_stack_depth));
            ptr::addr_of_mut!((*dst).max_loop_stack_depth_value)
                .write(encode_compact_count_u16(max_loop_stack_depth));
            core::ptr::copy_nonoverlapping(
                scratch.phases.as_ptr(),
                (*dst).phases.cast_mut(),
                phase_len,
            );
        }
        let eff_index_len = unsafe { (*dst).eff_index_to_step_len as usize };
        let mut eff_idx = 0usize;
        while eff_idx < eff_index_len {
            unsafe {
                (*dst)
                    .eff_index_to_step
                    .cast_mut()
                    .add(eff_idx)
                    .write(MACHINE_NO_STEP);
            }
            eff_idx += 1;
        }
        let copy_eff_index_len = core::cmp::min(eff_index_len, MAX_STEPS);
        unsafe {
            core::ptr::copy_nonoverlapping(
                scratch.eff_index_to_step.as_ptr(),
                (*dst).eff_index_to_step.cast_mut(),
                copy_eff_index_len,
            );
        }
        let step_state_len = unsafe { (*dst).step_index_to_state_len as usize };
        let mut step_idx = 0usize;
        while step_idx < step_state_len {
            unsafe {
                (*dst)
                    .step_index_to_state
                    .cast_mut()
                    .add(step_idx)
                    .write(StateIndex::MAX);
            }
            step_idx += 1;
        }
        let copy_step_state_len = core::cmp::min(step_state_len, MAX_STEPS);
        unsafe {
            core::ptr::copy_nonoverlapping(
                scratch.step_index_to_state.as_ptr(),
                (*dst).step_index_to_state.cast_mut(),
                copy_step_state_len,
            );
        }
    }

    #[cfg(test)]
    #[inline(never)]
    pub(crate) unsafe fn init_from_summary<const ROLE: u8>(
        dst: *mut Self,
        summary: &LoweringSummary,
        scratch: &mut RoleCompileScratch,
    ) {
        unsafe {
            Self::init_from_summary_with_layout::<ROLE>(
                dst,
                summary,
                scratch,
                None,
                summary.stamp().scope_count(),
                summary.stamp().scope_count(),
                summary.stamp().scope_count(),
            )
        };
    }

    #[inline(never)]
    pub(crate) unsafe fn init_from_summary_for_program<const ROLE: u8>(
        dst: *mut Self,
        summary: &LoweringSummary,
        scratch: &mut RoleCompileScratch,
        local_step_count: usize,
        passive_linger_route_scope_count: usize,
        route_scope_count: usize,
        parallel_enter_count: usize,
    ) {
        unsafe {
            Self::init_from_summary_with_layout::<ROLE>(
                dst,
                summary,
                scratch,
                Some(local_step_count),
                passive_linger_route_scope_count,
                route_scope_count,
                parallel_enter_count,
            )
        };
    }

    #[inline(never)]
    unsafe fn init_from_summary_with_layout<const ROLE: u8>(
        dst: *mut Self,
        summary: &LoweringSummary,
        scratch: &mut RoleCompileScratch,
        step_index_to_state_count: Option<usize>,
        passive_linger_route_scope_count: usize,
        route_scope_count: usize,
        parallel_enter_count: usize,
    ) {
        let init_empty =
            core::hint::black_box(init_empty_compiled_role_image as unsafe fn(*mut Self, u8));
        unsafe { init_empty(dst, ROLE) };
        let scope_count = summary.stamp().scope_count();
        let eff_count = summary.view().as_slice().len();
        let step_index_to_state_len = step_index_to_state_count.unwrap_or(eff_count);
        let storage = unsafe {
            CompiledRoleScopeStorage::from_image_ptr_with_layout(
                dst,
                scope_count,
                passive_linger_route_scope_count,
                route_scope_count,
                parallel_enter_count,
                eff_count,
                step_index_to_state_len,
            )
        };
        unsafe {
            ptr::addr_of_mut!((*dst).typestate).write(storage.typestate.cast_const());
            ptr::addr_of_mut!((*dst).phases).write(storage.phases.cast_const());
            ptr::addr_of_mut!((*dst).phase_len).write(encode_compact_count_u16(storage.phase_cap));
            ptr::addr_of_mut!((*dst).eff_index_to_step)
                .write(storage.eff_index_to_step.cast_const());
            ptr::addr_of_mut!((*dst).eff_index_to_step_len)
                .write(encode_compact_count_u16(eff_count));
            ptr::addr_of_mut!((*dst).step_index_to_state)
                .write(storage.step_index_to_state.cast_const());
            ptr::addr_of_mut!((*dst).step_index_to_state_len)
                .write(encode_compact_count_u16(step_index_to_state_len));
        }
        unsafe {
            crate::global::typestate::init_value_from_summary_for_role(
                storage.typestate,
                storage.typestate_nodes,
                storage.typestate_node_cap,
                ROLE,
                core::slice::from_raw_parts_mut(storage.records, storage.scope_cap),
                storage.slots_by_scope,
                storage.route_dense_by_slot,
                storage.route_records,
                storage.route_scope_cap,
                summary,
                scratch,
            );
        }
        let finalize = core::hint::black_box(
            finalize_compiled_role_image_from_typestate
                as unsafe fn(*mut Self, &mut RoleCompileScratch),
        );
        unsafe { finalize(dst, scratch) };
    }

    #[inline(always)]
    pub(crate) const fn role(&self) -> u8 {
        self.role
    }

    #[inline(always)]
    pub(crate) fn local_len(&self) -> usize {
        self.step_index_to_state_len as usize
    }

    #[inline(always)]
    pub(crate) fn phase(&self, idx: usize) -> Option<Phase> {
        if idx >= self.phase_len() {
            return None;
        }
        Some(unsafe { *self.phases.add(idx) })
    }

    #[inline(always)]
    pub(crate) fn typestate_ref(&self) -> &RoleTypestateValue {
        debug_assert!(!self.typestate.is_null());
        unsafe { &*self.typestate }
    }

    #[inline(always)]
    pub(crate) fn eff_index_to_step(&self) -> &[u16] {
        unsafe {
            core::slice::from_raw_parts(self.eff_index_to_step, self.eff_index_to_step_len as usize)
        }
    }

    #[inline(always)]
    pub(crate) fn step_index_to_state(&self) -> &[StateIndex] {
        unsafe {
            core::slice::from_raw_parts(
                self.step_index_to_state,
                self.step_index_to_state_len as usize,
            )
        }
    }

    #[inline(always)]
    pub(crate) fn active_lane_mask(&self) -> u8 {
        self.active_lane_mask_bits
    }

    #[inline(always)]
    pub(crate) fn fill_active_lane_dense_by_lane(&self, dst: &mut [u8; MAX_LANES]) -> usize {
        Self::build_active_lane_dense_map_into(self.active_lane_mask(), dst)
    }

    #[inline(always)]
    pub(crate) fn fill_logical_lane_dense_by_lane(&self, dst: &mut [u8; MAX_LANES]) -> usize {
        Self::build_logical_lane_dense_map_into(self.logical_lane_count(), dst)
    }

    #[inline(always)]
    pub(crate) fn logical_lane_count(&self) -> usize {
        self.logical_lane_count_value as usize
    }

    #[inline(always)]
    pub(crate) fn endpoint_lane_slot_count(&self) -> usize {
        self.endpoint_lane_slot_count_value as usize
    }

    #[inline(always)]
    pub(crate) fn max_route_stack_depth(&self) -> usize {
        self.max_route_stack_depth_value as usize
    }

    #[inline(always)]
    pub(crate) fn max_loop_stack_depth(&self) -> usize {
        self.max_loop_stack_depth_value as usize
    }

    #[inline(always)]
    pub(crate) fn route_table_frame_slots(&self) -> usize {
        core::cmp::max(
            self.max_route_stack_depth(),
            usize::from(self.route_scope_count() != 0),
        )
    }

    #[inline(always)]
    pub(crate) fn route_table_lane_slots(&self) -> usize {
        if self.route_table_frame_slots() == 0 {
            0
        } else {
            self.endpoint_lane_slot_count()
        }
    }

    #[inline(always)]
    pub(crate) fn loop_table_slots(&self) -> usize {
        self.max_loop_stack_depth()
    }

    #[inline(always)]
    pub(crate) fn resident_cap_entries(&self) -> usize {
        self.active_lane_count().saturating_mul(4).max(4)
    }

    #[inline(always)]
    pub(crate) fn max_frontier_entries(&self) -> usize {
        let compiled = self.compiled_frontier_entry_capacity();
        if cfg!(test) {
            test_frontier_entry_capacity(compiled)
        } else {
            compiled
        }
    }

    #[inline(always)]
    pub(crate) fn route_scope_count(&self) -> usize {
        self.route_scope_count_value as usize
    }

    #[inline(always)]
    pub(crate) fn scope_evidence_count(&self) -> usize {
        self.route_scope_count_value as usize
    }

    #[inline(always)]
    pub(crate) fn endpoint_arena_layout_for_binding(
        &self,
        binding_enabled: bool,
    ) -> EndpointArenaLayout {
        #[cfg(test)]
        let max_frontier_entries = self.compiled_max_frontier_entries();
        #[cfg(not(test))]
        let max_frontier_entries = self.max_frontier_entries();
        EndpointArenaLayout::new(
            self.active_lane_count(),
            if binding_enabled {
                self.logical_lane_count()
            } else {
                0
            },
            self.max_route_stack_depth(),
            self.scope_evidence_count(),
            max_frontier_entries,
        )
    }

    #[inline(always)]
    pub(crate) fn frontier_scratch_layout(&self) -> FrontierScratchLayout {
        if cfg!(test) {
            FrontierScratchLayout::new(test_frontier_entry_capacity(self.max_frontier_entries()))
        } else {
            FrontierScratchLayout::new(self.max_frontier_entries())
        }
    }

    #[cfg(test)]
    #[inline(always)]
    pub(crate) fn compiled_max_frontier_entries(&self) -> usize {
        self.compiled_frontier_entry_capacity()
    }

    #[cfg(test)]
    #[inline(always)]
    pub(crate) fn compiled_frontier_scratch_layout(&self) -> FrontierScratchLayout {
        FrontierScratchLayout::new(self.compiled_max_frontier_entries())
    }

    #[inline(always)]
    pub(crate) fn active_lane_count(&self) -> usize {
        self.active_lane_count_value as usize
    }

    #[inline(always)]
    fn phase_len(&self) -> usize {
        self.phase_len as usize
    }

    #[inline(always)]
    fn compiled_frontier_entry_capacity(&self) -> usize {
        self.compiled_frontier_entries as usize
    }

    fn build_active_lane_dense_map_into(active_lane_mask: u8, dst: &mut [u8; MAX_LANES]) -> usize {
        let mut lane_idx = 0usize;
        let mut dense = 0usize;
        while lane_idx < MAX_LANES {
            if ((active_lane_mask >> lane_idx) & 1) != 0 {
                dst[lane_idx] = dense as u8;
                dense += 1;
            } else {
                dst[lane_idx] = u8::MAX;
            }
            lane_idx += 1;
        }
        dense
    }

    fn binding_lane_count(active_lane_count: usize) -> usize {
        core::cmp::min(
            MAX_LANES,
            active_lane_count.saturating_add(RESERVED_BINDING_LANES),
        )
    }

    fn endpoint_lane_slot_count_from_mask(active_lane_mask: u8) -> usize {
        let live_lane_mask = active_lane_mask | 1;
        if live_lane_mask == 0 {
            0
        } else {
            core::cmp::min(
                MAX_LANES,
                (u8::BITS as usize).saturating_sub(live_lane_mask.leading_zeros() as usize),
            )
        }
    }

    fn build_logical_lane_dense_map_into(
        logical_lane_count: usize,
        dst: &mut [u8; MAX_LANES],
    ) -> usize {
        let mut lane_idx = 0usize;
        while lane_idx < MAX_LANES {
            dst[lane_idx] = if lane_idx < logical_lane_count {
                lane_idx as u8
            } else {
                u8::MAX
            };
            lane_idx += 1;
        }
        logical_lane_count
    }
}

#[cfg(test)]
mod tests {
    use core::{cell::UnsafeCell, mem::MaybeUninit};
    use std::thread_local;

    extern crate self as hibana;

    mod fanout_program {
        extern crate self as hibana;
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/internal/pico_smoke/src/fanout_program.rs"
        ));
    }
    mod huge_program {
        extern crate self as hibana;
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/internal/pico_smoke/src/huge_program.rs"
        ));
    }
    mod linear_program {
        extern crate self as hibana;
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/internal/pico_smoke/src/linear_program.rs"
        ));
    }
    mod route_control_kinds {
        extern crate self as hibana;
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/internal/pico_smoke/src/route_control_kinds.rs"
        ));
    }
    mod scenario {
        extern crate self as hibana;
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/internal/pico_smoke/src/scenario.rs"
        ));
    }

    fn retain_pico_smoke_fixture_symbols() {
        let _ = fanout_program::ROUTE_SCOPE_COUNT;
        let _ = fanout_program::EXPECTED_WORKER_BRANCH_LABELS;
        let _ = fanout_program::ACK_LABELS;
        let _ = fanout_program::run::<scenario::FixtureHarness>;
        let _ = huge_program::ROUTE_SCOPE_COUNT;
        let _ = huge_program::EXPECTED_WORKER_BRANCH_LABELS;
        let _ = huge_program::ACK_LABELS;
        let _ = huge_program::run::<scenario::FixtureHarness>;
        let _ = linear_program::ROUTE_SCOPE_COUNT;
        let _ = linear_program::EXPECTED_WORKER_BRANCH_LABELS;
        let _ = linear_program::ACK_LABELS;
        let _ = linear_program::run::<scenario::FixtureHarness>;
    }

    #[test]
    fn pico_smoke_fixture_symbols_are_reachable() {
        retain_pico_smoke_fixture_symbols();
    }

    use super::{CompiledRole, CompiledRoleImage, LoweringSummary};
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
        global::{
            CanonicalControl, ControlHandling, role_program,
            steps::{RouteSteps, SendStep, SeqSteps, StepCons, StepNil},
            typestate::{JumpReason, LocalAction, RoleCompileScratch},
        },
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

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct CheckpointKind;

    impl ResourceKind for CheckpointKind {
        type Handle = ();
        const TAG: u8 = 242;
        const NAME: &'static str = "CheckpointKind";
        const AUTO_MINT_EXTERNAL: bool = false;

        fn encode_handle(_handle: &Self::Handle) -> [u8; CAP_HANDLE_LEN] {
            [0u8; CAP_HANDLE_LEN]
        }

        fn decode_handle(_data: [u8; CAP_HANDLE_LEN]) -> Result<Self::Handle, CapError> {
            Ok(())
        }

        fn zeroize(_handle: &mut Self::Handle) {}

        fn caps_mask(_handle: &Self::Handle) -> CapsMask {
            CapsMask::empty()
        }

        fn scope_id(_handle: &Self::Handle) -> Option<crate::global::const_dsl::ScopeId> {
            None
        }
    }

    impl SessionScopedKind for CheckpointKind {
        fn handle_for_session(_sid: SessionId, _lane: Lane) -> Self::Handle {}

        fn shot() -> CapShot {
            CapShot::One
        }
    }

    impl ControlResourceKind for CheckpointKind {
        const LABEL: u8 = 0x52;
        const SCOPE: crate::global::const_dsl::ControlScopeKind =
            crate::global::const_dsl::ControlScopeKind::Checkpoint;
        const TAP_ID: u16 = 0x0400;
        const SHOT: CapShot = CapShot::One;
        const HANDLING: ControlHandling = ControlHandling::Canonical;
    }

    impl ControlMint for CheckpointKind {
        fn mint_handle(
            _sid: SessionId,
            _lane: Lane,
            _scope: crate::global::const_dsl::ScopeId,
        ) -> Self::Handle {
        }
    }

    type SendOnly<const LANE: u8, S, D, M> = StepCons<SendStep<S, D, M, LANE>, StepNil>;
    type BranchSteps<L, R> = RouteSteps<L, R>;

    const COMPILED_ROLE_IMAGE_BYTES: usize =
        super::CompiledRoleScopeStorage::total_bytes_for_counts(
            crate::eff::meta::MAX_EFF_NODES,
            crate::eff::meta::MAX_EFF_NODES,
            crate::eff::meta::MAX_EFF_NODES,
        );
    const COMPILED_ROLE_IMAGE_ALIGN: usize = super::CompiledRoleScopeStorage::overall_align();
    const COMPILED_ROLE_IMAGE_STORAGE_BYTES: usize =
        COMPILED_ROLE_IMAGE_BYTES + COMPILED_ROLE_IMAGE_ALIGN;

    thread_local! {
        static COMPILED_ROLE_STORAGE: UnsafeCell<MaybeUninit<CompiledRole>> =
            const { UnsafeCell::new(MaybeUninit::uninit()) };
        static COMPILED_ROLE_IMAGE_STORAGE: UnsafeCell<[u8; COMPILED_ROLE_IMAGE_STORAGE_BYTES]> =
            const { UnsafeCell::new([0u8; COMPILED_ROLE_IMAGE_STORAGE_BYTES]) };
        static COMPILED_ROLE_SCRATCH: UnsafeCell<MaybeUninit<RoleCompileScratch>> =
            const { UnsafeCell::new(MaybeUninit::uninit()) };
    }

    fn with_compiled_role<const ROLE: u8, GlobalSteps, R>(
        program: &role_program::RoleProgram<'_, ROLE, GlobalSteps, MintConfig>,
        f: impl FnOnce(&CompiledRole) -> R,
    ) -> R {
        crate::global::compiled::with_compiled_role_in_slot::<ROLE, _>(
            &COMPILED_ROLE_STORAGE,
            &COMPILED_ROLE_SCRATCH,
            crate::global::lowering_input(program),
            f,
        )
    }

    fn with_compiled_role_image<const ROLE: u8, GlobalSteps, R>(
        program: &role_program::RoleProgram<'_, ROLE, GlobalSteps, MintConfig>,
        f: impl FnOnce(&CompiledRoleImage) -> R,
    ) -> R {
        let lowering = crate::global::lowering_input(program);
        COMPILED_ROLE_IMAGE_STORAGE.with(|compiled| {
            COMPILED_ROLE_SCRATCH.with(|scratch| unsafe {
                let base = (*compiled.get()).as_mut_ptr() as usize;
                let compiled_ptr = ((base + COMPILED_ROLE_IMAGE_ALIGN - 1)
                    & !(COMPILED_ROLE_IMAGE_ALIGN - 1))
                    as *mut CompiledRoleImage;
                debug_assert!(
                    (compiled_ptr as usize) + COMPILED_ROLE_IMAGE_BYTES
                        <= base + COMPILED_ROLE_IMAGE_STORAGE_BYTES
                );
                let scratch_ptr = (*scratch.get()).as_mut_ptr();
                crate::global::compiled::with_compiled_role_image::<ROLE, _>(
                    compiled_ptr,
                    lowering,
                    scratch_ptr,
                    f,
                )
            })
        })
    }

    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    struct TypestateNodeStats {
        node_count: usize,
        send_count: usize,
        recv_count: usize,
        local_count: usize,
        jump_count: usize,
        terminate_count: usize,
        route_arm_end_jumps: usize,
        loop_continue_jumps: usize,
        loop_break_jumps: usize,
        passive_observer_branch_jumps: usize,
    }

    fn typestate_node_stats(image: &CompiledRoleImage) -> TypestateNodeStats {
        let typestate = image.typestate_ref();
        let mut stats = TypestateNodeStats::default();
        let mut idx = 0usize;
        while idx < typestate.len() {
            let node = typestate.node(idx);
            stats.node_count += 1;
            match node.action() {
                LocalAction::Send { .. } => stats.send_count += 1,
                LocalAction::Recv { .. } => stats.recv_count += 1,
                LocalAction::Local { .. } => stats.local_count += 1,
                LocalAction::Terminate => stats.terminate_count += 1,
                LocalAction::Jump { reason } => {
                    stats.jump_count += 1;
                    match reason {
                        JumpReason::RouteArmEnd => stats.route_arm_end_jumps += 1,
                        JumpReason::LoopContinue => stats.loop_continue_jumps += 1,
                        JumpReason::LoopBreak => stats.loop_break_jumps += 1,
                        JumpReason::PassiveObserverBranch => {
                            stats.passive_observer_branch_jumps += 1;
                        }
                    }
                }
            }
            idx += 1;
        }
        stats
    }

    #[test]
    fn compiled_role_exposes_controller_arm_and_dispatch_tables() {
        type LeftSteps = SeqSteps<
            SendOnly<
                0,
                Role<0>,
                Role<0>,
                Msg<
                    { crate::runtime::consts::LABEL_ROUTE_DECISION },
                    GenericCapToken<RouteDecisionKind>,
                    CanonicalControl<RouteDecisionKind>,
                >,
            >,
            SendOnly<0, Role<0>, Role<1>, Msg<41, ()>>,
        >;
        type RightSteps = SeqSteps<
            SendOnly<
                0,
                Role<0>,
                Role<0>,
                Msg<99, GenericCapToken<RouteRightKind>, CanonicalControl<RouteRightKind>>,
            >,
            SendOnly<0, Role<0>, Role<1>, Msg<47, ()>>,
        >;
        type ProgramSteps = BranchSteps<LeftSteps, RightSteps>;

        const LEFT: g::Program<LeftSteps> = g::seq(
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
        const RIGHT: g::Program<RightSteps> = g::seq(
            g::send::<
                Role<0>,
                Role<0>,
                Msg<99, GenericCapToken<RouteRightKind>, CanonicalControl<RouteRightKind>>,
                0,
            >(),
            g::send::<Role<0>, Role<1>, Msg<47, ()>, 0>(),
        );
        const PROGRAM: g::Program<ProgramSteps> = g::route(LEFT, RIGHT);
        let program = PROGRAM;

        let controller: crate::g::advanced::RoleProgram<'_, 0, _, MintConfig> =
            role_program::project(&program);
        with_compiled_role(&controller, |controller_compiled| {
            let controller_scope = controller_compiled.typestate_ref().node(0).scope();
            assert_eq!(controller_compiled.role(), 0);
            assert!(controller_compiled.is_active_lane(0));
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
            assert!(
                controller_compiled
                    .typestate_ref()
                    .controller_arm_entry_by_arm(controller_scope, 0)
                    .is_some(),
                "compiled role typestate must remain the single source of controller-arm facts"
            );
        });

        let worker: crate::g::advanced::RoleProgram<'_, 1, _, MintConfig> =
            role_program::project(&program);
        with_compiled_role(&worker, |worker_compiled| {
            let worker_scope = worker_compiled.typestate_ref().node(0).scope();
            assert_eq!(worker_compiled.role(), 1);
            assert!(worker_compiled.is_active_lane(0));
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
            assert!(
                worker_compiled
                    .typestate_ref()
                    .first_recv_dispatch_entry(worker_scope, 0)
                    .is_some(),
                "compiled role typestate must remain the single source of first-recv dispatch facts"
            );
            assert!(worker_compiled.step_for_eff_index(0).is_some());
            assert!(worker_compiled.state_for_step_index(0).is_some());
        });
    }

    #[test]
    fn large_route_prefix_keeps_offer_and_frontier_bounds_local() {
        type Prefix01 = StepCons<SendStep<Role<0>, Role<1>, Msg<1, u8>, 0>, StepNil>;
        type Prefix02 = StepCons<SendStep<Role<1>, Role<0>, Msg<2, u8>, 0>, StepNil>;
        type Prefix03 = StepCons<SendStep<Role<0>, Role<1>, Msg<3, u8>, 0>, StepNil>;
        type Prefix04 = StepCons<SendStep<Role<1>, Role<0>, Msg<4, u8>, 0>, StepNil>;
        type Prefix05 = StepCons<SendStep<Role<0>, Role<1>, Msg<5, u8>, 0>, StepNil>;
        type Prefix06 = StepCons<SendStep<Role<1>, Role<0>, Msg<6, u8>, 0>, StepNil>;
        type Prefix07 = StepCons<SendStep<Role<0>, Role<1>, Msg<7, u8>, 0>, StepNil>;
        type Prefix08 = StepCons<SendStep<Role<1>, Role<0>, Msg<8, u8>, 0>, StepNil>;
        type Prefix09 = StepCons<SendStep<Role<0>, Role<1>, Msg<9, u8>, 0>, StepNil>;
        type Prefix10 = StepCons<SendStep<Role<1>, Role<0>, Msg<10, u8>, 0>, StepNil>;
        type Prefix11 = StepCons<SendStep<Role<0>, Role<1>, Msg<11, u8>, 0>, StepNil>;
        type Prefix12 = StepCons<SendStep<Role<1>, Role<0>, Msg<12, u8>, 0>, StepNil>;
        type Prefix13 = StepCons<SendStep<Role<0>, Role<1>, Msg<13, u8>, 0>, StepNil>;
        type Prefix14 = StepCons<SendStep<Role<1>, Role<0>, Msg<14, u8>, 0>, StepNil>;
        type Prefix15 = StepCons<SendStep<Role<0>, Role<1>, Msg<15, u8>, 0>, StepNil>;
        type Prefix16 = StepCons<SendStep<Role<1>, Role<0>, Msg<16, u8>, 0>, StepNil>;
        type PrefixSteps = SeqSteps<
            Prefix01,
            SeqSteps<
                Prefix02,
                SeqSteps<
                    Prefix03,
                    SeqSteps<
                        Prefix04,
                        SeqSteps<
                            Prefix05,
                            SeqSteps<
                                Prefix06,
                                SeqSteps<
                                    Prefix07,
                                    SeqSteps<
                                        Prefix08,
                                        SeqSteps<
                                            Prefix09,
                                            SeqSteps<
                                                Prefix10,
                                                SeqSteps<
                                                    Prefix11,
                                                    SeqSteps<
                                                        Prefix12,
                                                        SeqSteps<
                                                            Prefix13,
                                                            SeqSteps<
                                                                Prefix14,
                                                                SeqSteps<Prefix15, Prefix16>,
                                                            >,
                                                        >,
                                                    >,
                                                >,
                                            >,
                                        >,
                                    >,
                                >,
                            >,
                        >,
                    >,
                >,
            >,
        >;
        type LeftSteps = SeqSteps<
            StepCons<
                SendStep<
                    Role<0>,
                    Role<0>,
                    Msg<
                        { crate::runtime::consts::LABEL_ROUTE_DECISION },
                        GenericCapToken<RouteDecisionKind>,
                        CanonicalControl<RouteDecisionKind>,
                    >,
                    0,
                >,
                StepNil,
            >,
            StepCons<SendStep<Role<0>, Role<1>, Msg<41, ()>, 0>, StepNil>,
        >;
        type RightSteps = SeqSteps<
            StepCons<
                SendStep<
                    Role<0>,
                    Role<0>,
                    Msg<99, GenericCapToken<RouteRightKind>, CanonicalControl<RouteRightKind>>,
                    0,
                >,
                StepNil,
            >,
            StepCons<SendStep<Role<0>, Role<1>, Msg<47, ()>, 0>, StepNil>,
        >;
        type ProgramSteps = SeqSteps<PrefixSteps, RouteSteps<LeftSteps, RightSteps>>;

        const PREFIX: crate::g::Program<PrefixSteps> = g::seq(
            g::send::<Role<0>, Role<1>, Msg<1, u8>, 0>(),
            g::seq(
                g::send::<Role<1>, Role<0>, Msg<2, u8>, 0>(),
                g::seq(
                    g::send::<Role<0>, Role<1>, Msg<3, u8>, 0>(),
                    g::seq(
                        g::send::<Role<1>, Role<0>, Msg<4, u8>, 0>(),
                        g::seq(
                            g::send::<Role<0>, Role<1>, Msg<5, u8>, 0>(),
                            g::seq(
                                g::send::<Role<1>, Role<0>, Msg<6, u8>, 0>(),
                                g::seq(
                                    g::send::<Role<0>, Role<1>, Msg<7, u8>, 0>(),
                                    g::seq(
                                        g::send::<Role<1>, Role<0>, Msg<8, u8>, 0>(),
                                        g::seq(
                                            g::send::<Role<0>, Role<1>, Msg<9, u8>, 0>(),
                                            g::seq(
                                                g::send::<Role<1>, Role<0>, Msg<10, u8>, 0>(),
                                                g::seq(
                                                    g::send::<Role<0>, Role<1>, Msg<11, u8>, 0>(),
                                                    g::seq(
                                                        g::send::<Role<1>, Role<0>, Msg<12, u8>, 0>(
                                                        ),
                                                        g::seq(
                                                            g::send::<
                                                                Role<0>,
                                                                Role<1>,
                                                                Msg<13, u8>,
                                                                0,
                                                            >(
                                                            ),
                                                            g::seq(
                                                                g::send::<
                                                                    Role<1>,
                                                                    Role<0>,
                                                                    Msg<14, u8>,
                                                                    0,
                                                                >(
                                                                ),
                                                                g::seq(
                                                                    g::send::<
                                                                        Role<0>,
                                                                        Role<1>,
                                                                        Msg<15, u8>,
                                                                        0,
                                                                    >(
                                                                    ),
                                                                    g::send::<
                                                                        Role<1>,
                                                                        Role<0>,
                                                                        Msg<16, u8>,
                                                                        0,
                                                                    >(
                                                                    ),
                                                                ),
                                                            ),
                                                        ),
                                                    ),
                                                ),
                                            ),
                                        ),
                                    ),
                                ),
                            ),
                        ),
                    ),
                ),
            ),
        );
        const LEFT: crate::g::Program<LeftSteps> = g::seq(
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
        const RIGHT: crate::g::Program<RightSteps> = g::seq(
            g::send::<
                Role<0>,
                Role<0>,
                Msg<99, GenericCapToken<RouteRightKind>, CanonicalControl<RouteRightKind>>,
                0,
            >(),
            g::send::<Role<0>, Role<1>, Msg<47, ()>, 0>(),
        );
        const PROGRAM: crate::g::Program<ProgramSteps> = g::seq(PREFIX, g::route(LEFT, RIGHT));
        let program = PROGRAM;

        let worker: crate::g::advanced::RoleProgram<'_, 1, _, MintConfig> =
            role_program::project(&program);
        let lowering = crate::global::lowering_input(&worker);
        let summary = lowering.summary();
        assert!(
            CompiledRoleImage::persistent_bytes_for_program(
                summary.stamp().scope_count(),
                lowering.passive_linger_route_scope_count(),
                lowering.route_scope_count(),
                lowering.parallel_enter_count(),
                lowering.eff_count(),
                lowering.local_step_count(),
            ) < CompiledRoleImage::persistent_bytes_for_counts(
                summary.stamp().scope_count(),
                lowering.route_scope_count(),
                lowering.eff_count(),
            ),
            "role image sizing should use the projected local step count instead of full eff_count"
        );
        with_compiled_role_image(&worker, |image| {
            assert!(
                image.local_len() >= 9,
                "large prefix should still project a substantial local program"
            );
            assert_eq!(
                image.route_scope_count(),
                1,
                "single trailing route should compile to one route scope"
            );
            assert_eq!(
                image.compiled_max_frontier_entries(),
                1,
                "frontier bound must stay tied to the active route frontier"
            );
            assert!(
                image.compiled_frontier_scratch_layout().total_bytes()
                    < image.local_len()
                        * core::mem::size_of::<crate::global::typestate::StateIndex>()
                        * 8,
                "frontier scratch must stay local to route metadata instead of scaling with the full local program"
            );
        });
    }

    fn assert_huge_shape_bounds<Steps>(
        program: &crate::g::Program<Steps>,
        expected_route_scope_count: usize,
        expected_frontier_entries: usize,
    ) where
        Steps: crate::global::program::BuildProgramSource
            + crate::g::advanced::steps::ProjectRole<crate::g::Role<1>>,
    {
        let worker: crate::g::advanced::RoleProgram<'_, 1, _, MintConfig> =
            role_program::project(program);
        with_compiled_role_image(&worker, |image| {
            let active_lane_count = image.active_lane_count();
            let layout = image.endpoint_arena_layout_for_binding(true);
            let no_binding_layout = image.endpoint_arena_layout_for_binding(false);

            assert!(
                image.local_len() >= expected_route_scope_count,
                "huge choreography local length must dominate the route scope count"
            );
            assert_eq!(
                image.route_scope_count(),
                expected_route_scope_count,
                "route scope count must stay tied to the huge choreography shape"
            );
            assert_eq!(
                image.compiled_max_frontier_entries(),
                expected_frontier_entries,
                "frontier bound must stay tied to branch-local fan-out"
            );
            assert!(
                image.compiled_max_frontier_entries() < image.local_len().max(1),
                "frontier bound must not grow with the full local prefix"
            );
            assert_eq!(
                layout.scope_evidence_slots().count(),
                image.scope_evidence_count(),
                "scope evidence storage must stay exact-bound to the compiled evidence count"
            );
            assert_eq!(
                layout.binding_slots().count(),
                image.logical_lane_count() * 8,
                "binding storage must stay exact-bound to the logical lane count"
            );
            assert_eq!(
                no_binding_layout.binding_slots().count(),
                0,
                "NoBinding layout must not reserve buffered binding slots"
            );
            assert_eq!(
                no_binding_layout.binding_len().count(),
                0,
                "NoBinding layout must not reserve binding len storage"
            );
            assert_eq!(
                no_binding_layout.binding_label_masks().count(),
                0,
                "NoBinding layout must not reserve binding label masks"
            );
            assert!(
                no_binding_layout.total_bytes() < layout.total_bytes(),
                "NoBinding layout must stay smaller than the binding-capable layout"
            );
            assert_eq!(
                layout.route_arm_stack().count(),
                active_lane_count * image.max_route_stack_depth(),
                "route-arm stack must stay exact-bound to active lanes and route depth"
            );
            assert_eq!(
                layout.frontier_offer_entry_slots().count(),
                image.max_frontier_entries(),
                "offer entry storage must stay tied to the test-visible simultaneous frontier bound"
            );
        });
    }

    fn count_parallel_enter_markers(summary: &LoweringSummary) -> usize {
        let markers = summary.view().scope_markers();
        let mut count = 0usize;
        let mut idx = 0usize;
        while idx < markers.len() {
            let marker = markers[idx];
            if matches!(marker.event, crate::global::const_dsl::ScopeEvent::Enter)
                && matches!(
                    marker.scope_kind,
                    crate::global::const_dsl::ScopeKind::Parallel
                )
            {
                count += 1;
            }
            idx += 1;
        }
        count
    }

    #[test]
    fn huge_shape_phase_counts_stay_bounded_by_parallel_markers() {
        let route_program = huge_program::PROGRAM;
        let route_worker: crate::g::advanced::RoleProgram<'_, 1, _, MintConfig> =
            role_program::project(&route_program);
        let route_lowering = crate::global::lowering_input(&route_worker);
        let route_summary = route_lowering.summary();
        let route_parallel_markers = count_parallel_enter_markers(route_summary);
        with_compiled_role_image(&route_worker, |image| {
            let phase_count = image.phase_len();
            let local_len = image.local_len();
            let bound = if local_len == 0 {
                0
            } else {
                route_parallel_markers.saturating_mul(2).saturating_add(1)
            };
            assert!(
                phase_count <= bound,
                "route-heavy phase count must stay bounded by parallel enter markers"
            );
            std::println!(
                "phase-shape name=route_heavy local_len={} phase_count={} parallel_enter_markers={} phase_size={} lane_steps_size={} route_guard_size={}",
                local_len,
                phase_count,
                route_parallel_markers,
                core::mem::size_of::<crate::global::role_program::Phase>(),
                core::mem::size_of::<crate::global::role_program::LaneSteps>(),
                core::mem::size_of::<crate::global::role_program::PhaseRouteGuard>(),
            );
        });

        let linear_program = linear_program::PROGRAM;
        let linear_worker: crate::g::advanced::RoleProgram<'_, 1, _, MintConfig> =
            role_program::project(&linear_program);
        let linear_lowering = crate::global::lowering_input(&linear_worker);
        let linear_summary = linear_lowering.summary();
        let linear_parallel_markers = count_parallel_enter_markers(linear_summary);
        with_compiled_role_image(&linear_worker, |image| {
            let phase_count = image.phase_len();
            let local_len = image.local_len();
            let bound = if local_len == 0 {
                0
            } else {
                linear_parallel_markers.saturating_mul(2).saturating_add(1)
            };
            assert!(
                phase_count <= bound,
                "linear-heavy phase count must stay bounded by parallel enter markers"
            );
            std::println!(
                "phase-shape name=linear_heavy local_len={} phase_count={} parallel_enter_markers={}",
                local_len,
                phase_count,
                linear_parallel_markers,
            );
        });

        let fanout_program = fanout_program::PROGRAM;
        let fanout_worker: crate::g::advanced::RoleProgram<'_, 1, _, MintConfig> =
            role_program::project(&fanout_program);
        let fanout_lowering = crate::global::lowering_input(&fanout_worker);
        let fanout_summary = fanout_lowering.summary();
        let fanout_parallel_markers = count_parallel_enter_markers(fanout_summary);
        with_compiled_role_image(&fanout_worker, |image| {
            let phase_count = image.phase_len();
            let local_len = image.local_len();
            let bound = if local_len == 0 {
                0
            } else {
                fanout_parallel_markers.saturating_mul(2).saturating_add(1)
            };
            assert!(
                phase_count <= bound,
                "fanout-heavy phase count must stay bounded by parallel enter markers"
            );
            std::println!(
                "phase-shape name=fanout_heavy local_len={} phase_count={} parallel_enter_markers={}",
                local_len,
                phase_count,
                fanout_parallel_markers,
            );
        });
    }

    fn print_role_tail_breakdown<const ROLE: u8, Steps>(
        name: &str,
        program: &crate::g::Program<Steps>,
    ) where
        Steps: crate::global::program::BuildProgramSource
            + crate::g::advanced::steps::ProjectRole<crate::g::Role<ROLE>>,
        <Steps as crate::g::advanced::steps::ProjectRole<crate::g::Role<ROLE>>>::Output:
            crate::global::steps::StepCount,
    {
        let worker: crate::g::advanced::RoleProgram<'_, ROLE, _, MintConfig> =
            role_program::project(program);
        let lowering = crate::global::lowering_input(&worker);
        let summary = lowering.summary();
        let scope_count = summary.stamp().scope_count();
        let eff_count = lowering.eff_count();
        let route_enter_count = summary
            .view()
            .scope_markers()
            .iter()
            .filter(|marker| {
                matches!(marker.event, crate::global::const_dsl::ScopeEvent::Enter)
                    && matches!(
                        marker.scope_kind,
                        crate::global::const_dsl::ScopeKind::Route
                    )
            })
            .count();
        let local_len = lowering.local_step_count();
        let phase_cap =
            super::CompiledRoleScopeStorage::phase_cap(local_len, lowering.parallel_enter_count());
        let typestate_node_cap = super::CompiledRoleScopeStorage::typestate_node_cap(
            scope_count,
            lowering.passive_linger_route_scope_count(),
            local_len,
        );
        let scope_cap = super::CompiledRoleScopeStorage::scope_cap(scope_count);
        let route_scope_cap =
            super::CompiledRoleScopeStorage::route_scope_cap(lowering.route_scope_count());
        let eff_cap = super::CompiledRoleScopeStorage::step_cap(eff_count);
        let step_cap = super::CompiledRoleScopeStorage::step_cap(local_len);
        let route_stats = with_compiled_role_image(&worker, |image| {
            image.typestate_ref().route_scope_payload_stats()
        });
        let scope_stats =
            with_compiled_role_image(&worker, |image| image.typestate_ref().scope_payload_stats());
        let node_stats = with_compiled_role_image(&worker, typestate_node_stats);
        std::println!(
            "role-tail-breakdown name={name} scope_count={} eff_count={} local_len={} phase_cap={} typestate_node_cap={} built_node_len={} typestate_node_slack={} local_node_size={} local_action_size={} policy_mode_size={} scope_record_size={} route_scope_record_size={} state_index_size={} typestate_nodes_bytes={} phases_bytes={} records_bytes={} slots_bytes={} route_dense_bytes={} route_records_bytes={} route_recv_bytes={} eff_index_bytes={} step_index_bytes={} total_bytes={} send_nodes={} recv_nodes={} local_nodes={} jump_nodes={} terminate_nodes={} route_arm_end_jumps={} loop_continue_jumps={} loop_break_jumps={} passive_observer_branch_jumps={} total_lane_first_entries={} max_lane_first_entries={} total_lane_last_entries={} max_lane_last_entries={} total_arm_entries={} max_arm_entries={} total_passive_arm_scopes={} max_passive_arm_scopes={} route_scope_count={} route_enter_count={} total_first_recv_entries={} max_first_recv_entries={} total_arm_lane_last_entries={} max_arm_lane_last_entries={} total_arm_lane_last_override_entries={} max_arm_lane_last_override_entries={} total_offer_lane_entries={} max_offer_lane_entries={}",
            scope_count,
            eff_count,
            local_len,
            phase_cap,
            typestate_node_cap,
            node_stats.node_count,
            typestate_node_cap.saturating_sub(node_stats.node_count),
            core::mem::size_of::<crate::global::typestate::LocalNode>(),
            crate::global::typestate::LocalNode::packed_action_size(),
            core::mem::size_of::<crate::global::const_dsl::PolicyMode>(),
            core::mem::size_of::<crate::global::typestate::ScopeRecord>(),
            core::mem::size_of::<crate::global::typestate::RouteScopeRecord>(),
            core::mem::size_of::<crate::global::typestate::StateIndex>(),
            typestate_node_cap * core::mem::size_of::<crate::global::typestate::LocalNode>(),
            phase_cap * core::mem::size_of::<crate::global::role_program::Phase>(),
            scope_cap * core::mem::size_of::<crate::global::typestate::ScopeRecord>(),
            scope_cap * core::mem::size_of::<u16>(),
            scope_cap * core::mem::size_of::<u16>(),
            route_scope_cap * core::mem::size_of::<crate::global::typestate::RouteScopeRecord>(),
            0usize,
            eff_cap * core::mem::size_of::<u16>(),
            step_cap * core::mem::size_of::<crate::global::typestate::StateIndex>(),
            CompiledRoleImage::persistent_bytes_for_program(
                summary.stamp().scope_count(),
                lowering.passive_linger_route_scope_count(),
                lowering.route_scope_count(),
                lowering.parallel_enter_count(),
                eff_count,
                local_len,
            ),
            node_stats.send_count,
            node_stats.recv_count,
            node_stats.local_count,
            node_stats.jump_count,
            node_stats.terminate_count,
            node_stats.route_arm_end_jumps,
            node_stats.loop_continue_jumps,
            node_stats.loop_break_jumps,
            node_stats.passive_observer_branch_jumps,
            scope_stats.total_lane_first_entries,
            scope_stats.max_lane_first_entries,
            scope_stats.total_lane_last_entries,
            scope_stats.max_lane_last_entries,
            scope_stats.total_arm_entries,
            scope_stats.max_arm_entries,
            scope_stats.total_passive_arm_scopes,
            scope_stats.max_passive_arm_scopes,
            route_stats.route_scope_count,
            route_enter_count,
            route_stats.total_first_recv_entries,
            route_stats.max_first_recv_entries,
            route_stats.total_arm_lane_last_entries,
            route_stats.max_arm_lane_last_entries,
            route_stats.total_arm_lane_last_override_entries,
            route_stats.max_arm_lane_last_override_entries,
            route_stats.total_offer_lane_entries,
            route_stats.max_offer_lane_entries,
        );
    }

    #[test]
    fn huge_shape_role_image_tail_breakdown_is_reported() {
        let route_program = huge_program::PROGRAM;
        print_role_tail_breakdown::<1, huge_program::ProgramSteps>("route_heavy", &route_program);

        let linear_program = linear_program::PROGRAM;
        print_role_tail_breakdown::<1, linear_program::ProgramSteps>(
            "linear_heavy",
            &linear_program,
        );

        let fanout_program = fanout_program::PROGRAM;
        print_role_tail_breakdown::<1, fanout_program::ProgramSteps>(
            "fanout_heavy",
            &fanout_program,
        );
    }

    #[test]
    fn offer_regression_role_tail_breakdown_is_reported() {
        type LoopContinueMsg = Msg<
            { crate::runtime::consts::LABEL_LOOP_CONTINUE },
            GenericCapToken<crate::control::cap::resource_kinds::LoopContinueKind>,
            CanonicalControl<crate::control::cap::resource_kinds::LoopContinueKind>,
        >;
        type LoopBreakMsg = Msg<
            { crate::runtime::consts::LABEL_LOOP_BREAK },
            GenericCapToken<crate::control::cap::resource_kinds::LoopBreakKind>,
            CanonicalControl<crate::control::cap::resource_kinds::LoopBreakKind>,
        >;
        type SessionRequestWireMsg = Msg<0x10, u8>;
        type AdminReplyMsg = Msg<0x50, u8>;
        type SnapshotCandidatesReplyMsg = Msg<0x51, u8>;
        type CheckpointMsg = Msg<
            { CheckpointKind::LABEL },
            GenericCapToken<CheckpointKind>,
            CanonicalControl<CheckpointKind>,
        >;
        type StaticRouteLeftMsg = Msg<
            { crate::runtime::consts::LABEL_ROUTE_DECISION },
            GenericCapToken<RouteDecisionKind>,
            CanonicalControl<RouteDecisionKind>,
        >;
        type StaticRouteRightMsg =
            Msg<99, GenericCapToken<RouteRightKind>, CanonicalControl<RouteRightKind>>;
        type ReplyDecisionLeftSteps = SeqSteps<
            SendOnly<3, Role<1>, Role<1>, StaticRouteLeftMsg>,
            SendOnly<3, Role<1>, Role<0>, AdminReplyMsg>,
        >;
        type SnapshotReplyPathSteps = SeqSteps<
            SendOnly<3, Role<1>, Role<1>, StaticRouteLeftMsg>,
            SeqSteps<
                SendOnly<3, Role<1>, Role<1>, StaticRouteLeftMsg>,
                SeqSteps<
                    SendOnly<3, Role<1>, Role<0>, SnapshotCandidatesReplyMsg>,
                    SendOnly<3, Role<0>, Role<0>, CheckpointMsg>,
                >,
            >,
        >;
        type ReplyDecisionRightSteps =
            SeqSteps<SendOnly<3, Role<1>, Role<1>, StaticRouteRightMsg>, SnapshotReplyPathSteps>;
        type ReplyDecisionSteps = BranchSteps<ReplyDecisionLeftSteps, ReplyDecisionRightSteps>;
        type RequestExchangeSteps =
            SeqSteps<SendOnly<3, Role<0>, Role<1>, SessionRequestWireMsg>, ReplyDecisionSteps>;
        type ContinueArmSteps =
            SeqSteps<SendOnly<3, Role<0>, Role<0>, LoopContinueMsg>, RequestExchangeSteps>;
        type BreakArmSteps = SendOnly<3, Role<0>, Role<0>, LoopBreakMsg>;
        type LoopProgramSteps = BranchSteps<ContinueArmSteps, BreakArmSteps>;

        const REPLY_DECISION: g::Program<ReplyDecisionSteps> = g::route(
            g::seq(
                g::send::<Role<1>, Role<1>, StaticRouteLeftMsg, 3>(),
                g::send::<Role<1>, Role<0>, AdminReplyMsg, 3>(),
            ),
            g::seq(
                g::send::<Role<1>, Role<1>, StaticRouteRightMsg, 3>(),
                g::seq(
                    g::send::<Role<1>, Role<1>, StaticRouteLeftMsg, 3>(),
                    g::seq(
                        g::send::<Role<1>, Role<1>, StaticRouteLeftMsg, 3>(),
                        g::seq(
                            g::send::<Role<1>, Role<0>, SnapshotCandidatesReplyMsg, 3>(),
                            g::send::<Role<0>, Role<0>, CheckpointMsg, 3>(),
                        ),
                    ),
                ),
            ),
        );
        const REQUEST_EXCHANGE: g::Program<RequestExchangeSteps> = g::seq(
            g::send::<Role<0>, Role<1>, SessionRequestWireMsg, 3>(),
            REPLY_DECISION,
        );
        const LOOP_PROGRAM: g::Program<LoopProgramSteps> = g::route(
            g::seq(
                g::send::<Role<0>, Role<0>, LoopContinueMsg, 3>(),
                REQUEST_EXCHANGE,
            ),
            g::send::<Role<0>, Role<0>, LoopBreakMsg, 3>(),
        );
        let program = LOOP_PROGRAM;

        print_role_tail_breakdown::<0, LoopProgramSteps>("offer_admin_snapshot_client", &program);
        print_role_tail_breakdown::<1, LoopProgramSteps>("offer_admin_snapshot_server", &program);
    }

    #[test]
    fn huge_route_heavy_shape_keeps_resident_bounds_local() {
        let program = huge_program::PROGRAM;
        assert_huge_shape_bounds(&program, huge_program::ROUTE_SCOPE_COUNT, 1);
    }

    #[test]
    fn huge_linear_heavy_shape_keeps_resident_bounds_local() {
        let program = linear_program::PROGRAM;
        assert_huge_shape_bounds(&program, linear_program::ROUTE_SCOPE_COUNT, 0);
    }

    #[test]
    fn huge_fanout_heavy_shape_keeps_resident_bounds_local() {
        let program = fanout_program::PROGRAM;
        assert_huge_shape_bounds(&program, fanout_program::ROUTE_SCOPE_COUNT, 1);
    }
}
