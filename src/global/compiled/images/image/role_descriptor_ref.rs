use super::{
    CompiledProgramRef, CompiledRoleImage, ControlSemanticKind, DENSE_LANE_NONE, DenseLaneOrdinal,
    EffIndex, EffKind, EndpointArenaLayout, LocalAtomFacts, LocalNode, LocalNodeMeta, PolicyMode,
    ScopeEvent, ScopeId, ScopeKind, ScopeRegion, StateIndex, first_enter_for_scope, same_scope,
};
mod route_scope;

#[derive(Clone, Copy)]
pub(crate) struct RoleDescriptorRef {
    program: CompiledProgramRef,
    resident: &'static CompiledRoleImage,
}

impl core::fmt::Debug for RoleDescriptorRef {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("RoleDescriptorRef")
            .field("program", &self.program)
            .field("role", &self.role())
            .finish()
    }
}

impl PartialEq for RoleDescriptorRef {
    #[inline(always)]
    fn eq(&self, other: &Self) -> bool {
        if self.program != other.program || self.role() != other.role() {
            return false;
        }
        core::ptr::eq(self.resident, other.resident)
    }
}

impl Eq for RoleDescriptorRef {}

impl RoleDescriptorRef {
    #[inline(always)]
    pub(crate) const fn from_resident(compiled: &'static CompiledRoleImage) -> Self {
        Self {
            program: compiled.program(),
            resident: compiled,
        }
    }

    #[inline(always)]
    pub(crate) const fn program(&self) -> CompiledProgramRef {
        self.program
    }

    #[inline(always)]
    const fn resident(&self) -> &'static CompiledRoleImage {
        self.resident
    }

    #[inline(always)]
    pub(crate) const fn local_event_rows(&self) -> crate::global::role_program::RoleImageRef {
        self.resident.role_image()
    }

    #[inline(always)]
    fn footprint(&self) -> crate::global::role_program::RoleFootprint {
        self.resident.footprint()
    }

    #[inline(always)]
    fn endpoint_layout_footprint(&self) -> crate::global::role_program::RoleFootprint {
        self.footprint()
    }

    #[inline(always)]
    pub(crate) fn role(&self) -> u8 {
        self.resident.role()
    }

    #[inline(always)]
    pub(crate) fn local_len(&self) -> usize {
        self.resident.footprint().local_step_count
    }

    #[inline(always)]
    pub(crate) fn node_len(&self) -> usize {
        self.local_len().saturating_add(1)
    }

    #[inline(always)]
    pub(crate) fn checked_node(&self, idx: usize) -> Option<LocalNode> {
        if idx >= self.node_len() {
            return None;
        }
        Some(self.node(idx))
    }

    #[inline(always)]
    pub(crate) fn node(&self, idx: usize) -> LocalNode {
        let compiled = self.resident();
        let role = compiled.role();
        self.resident_node(role, compiled, idx)
    }

    fn resident_node(&self, role: u8, compiled: &CompiledRoleImage, idx: usize) -> LocalNode {
        let local_len = self.local_len();
        if idx >= local_len {
            return LocalNode::terminal(StateIndex::from_usize(local_len));
        }
        let (eff_idx, action_ordinal) = self
            .resident_eff_for_step(role, compiled, idx)
            .expect("resident local step index must resolve to an effect");
        let view = compiled.program_image().view();
        let eff = view.node_at(eff_idx);
        let atom = eff.atom_data();
        let scope = self.resident_scope_at(compiled, eff_idx);
        let policy = match view.policy_at(eff_idx) {
            Some(policy) => policy.with_scope(scope),
            None => PolicyMode::Static,
        };
        let control_desc = if atom.is_control {
            view.control_desc_at(eff_idx)
        } else {
            None
        };
        let semantic = ControlSemanticKind::from_control_desc(control_desc);
        let shot = control_desc.map(|desc| desc.shot());
        let resource = atom.resource;
        let frame_label = self.resident_frame_label_at(compiled, eff_idx);
        let route_scope_and_arm = self.resident_route_scope_and_arm_at(compiled, eff_idx);
        let route_arm = route_scope_and_arm.map(|(_, arm)| arm);
        let next = StateIndex::from_usize(action_ordinal.saturating_add(1));
        let eff_index = EffIndex::from_dense_ordinal(eff_idx);
        let facts = LocalAtomFacts {
            eff_index,
            label: atom.label,
            frame_label,
            resource,
            is_control: atom.is_control,
            shot,
            policy,
            lane: atom.lane,
        };
        let meta = |is_choice_determinant| LocalNodeMeta {
            semantic,
            next,
            scope,
            route_arm,
            is_choice_determinant,
        };
        if atom.from == role && atom.to == role {
            LocalNode::local(facts, meta(false))
        } else if atom.from == role {
            LocalNode::send(atom.to, facts, meta(false))
        } else {
            LocalNode::recv(
                atom.from,
                facts,
                meta(route_scope_and_arm.is_some_and(|(route_scope, arm)| {
                    self.resident_first_recv_eff_for_route_arm(role, compiled, route_scope, arm)
                        == Some(eff_idx)
                })),
            )
        }
    }

    fn resident_eff_for_step(
        &self,
        role: u8,
        compiled: &CompiledRoleImage,
        target_step: usize,
    ) -> Option<(usize, usize)> {
        let view = compiled.program_image().view();
        let mut step = 0usize;
        let mut idx = 0usize;
        while idx < view.len() {
            let node = view.node_at(idx);
            if matches!(node.kind, EffKind::Atom) {
                let atom = node.atom_data();
                if atom.from == role || atom.to == role {
                    if step == target_step {
                        return Some((idx, step));
                    }
                    step += 1;
                }
            }
            idx += 1;
        }
        None
    }

    fn resident_step_for_eff(
        &self,
        role: u8,
        compiled: &CompiledRoleImage,
        target_eff: usize,
    ) -> Option<usize> {
        let view = compiled.program_image().view();
        let mut step = 0usize;
        let mut idx = 0usize;
        while idx < view.len() {
            let node = view.node_at(idx);
            if matches!(node.kind, EffKind::Atom) {
                let atom = node.atom_data();
                if atom.from == role || atom.to == role {
                    if idx == target_eff {
                        return Some(step);
                    }
                    step += 1;
                }
            }
            idx += 1;
        }
        None
    }
    #[inline(always)]
    pub(crate) fn has_active_lane(&self, lane_idx: usize) -> bool {
        if lane_idx >= self.logical_lane_count() {
            return false;
        }
        self.resident()
            .role_image()
            .active_lane_set()
            .contains(lane_idx)
    }

    #[inline(always)]
    pub(crate) fn first_active_lane(&self) -> Option<usize> {
        self.resident().role_image().first_active_lane()
    }

    #[inline(always)]
    pub(crate) fn endpoint_lane_slot_count(&self) -> usize {
        self.footprint().endpoint_lane_slot_count.max(1)
    }

    #[inline(always)]
    pub(crate) fn logical_lane_count(&self) -> usize {
        self.footprint()
            .logical_lane_count
            .max(self.endpoint_lane_slot_count())
    }

    #[inline(always)]
    pub(crate) fn route_table_frame_slots(&self) -> usize {
        if self.route_scope_count() == 0 {
            0
        } else {
            self.footprint()
                .active_lane_count
                .saturating_mul(self.max_route_stack_depth().max(1))
        }
    }

    #[inline(always)]
    pub(crate) fn route_table_lane_slots(&self) -> usize {
        if self.route_scope_count() == 0 {
            0
        } else {
            self.endpoint_lane_slot_count()
        }
    }

    #[inline(always)]
    pub(crate) fn loop_table_slots(&self) -> usize {
        self.endpoint_lane_slot_count()
            .saturating_mul(self.footprint().passive_linger_route_scope_count)
    }

    #[inline(always)]
    pub(crate) fn resident_cap_entries(&self) -> usize {
        self.footprint().active_lane_count.saturating_mul(4).max(4)
    }

    #[cfg(test)]
    #[inline(always)]
    pub(crate) fn active_lane_count(&self) -> usize {
        self.footprint().active_lane_count
    }

    #[inline(always)]
    pub(crate) fn max_route_stack_depth(&self) -> usize {
        self.footprint().max_route_stack_depth
    }

    #[cfg(test)]
    #[inline(always)]
    pub(crate) fn max_loop_stack_depth(&self) -> usize {
        self.footprint().passive_linger_route_scope_count
    }

    #[inline(always)]
    pub(crate) fn route_scope_count(&self) -> usize {
        self.footprint().route_scope_count
    }

    #[inline(always)]
    pub(crate) fn fill_active_lane_dense_by_lane(&self, dst: &mut [DenseLaneOrdinal]) -> usize {
        dst.fill(DENSE_LANE_NONE);
        let active = self.resident().role_image().active_lane_set();
        let mut dense = 0usize;
        let mut next = active.first_set(dst.len());
        while let Some(lane_idx) = next {
            dst[lane_idx] =
                DenseLaneOrdinal::new(dense).expect("dense active lane ordinal fits u16");
            dense += 1;
            next = active.next_set_from(lane_idx.saturating_add(1), dst.len());
        }
        dense
    }

    #[inline(always)]
    pub(crate) fn endpoint_arena_layout(&self) -> EndpointArenaLayout {
        EndpointArenaLayout::from_footprint(self.endpoint_layout_footprint())
    }
}
