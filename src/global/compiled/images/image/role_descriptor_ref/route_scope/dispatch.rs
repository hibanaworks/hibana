use super::RoleDescriptorRef;
use crate::{
    eff::EffKind,
    global::{
        const_dsl::ScopeId,
        typestate::{FirstRecvDispatchSpec, MAX_FIRST_RECV_DISPATCH, StateIndex},
    },
};

impl RoleDescriptorRef {
    #[inline(always)]
    pub(crate) fn controller_arm_entry_for_label(
        &self,
        scope_id: ScopeId,
        label: u8,
    ) -> Option<StateIndex> {
        let mut arm = 0u8;
        while arm < 2 {
            if let Some((entry, entry_label)) = self.controller_arm_entry_by_arm(scope_id, arm)
                && entry_label == label
            {
                return Some(entry);
            }
            arm += 1;
        }
        None
    }

    #[inline(always)]
    pub(crate) fn controller_arm_entry_by_arm(
        &self,
        scope_id: ScopeId,
        arm: u8,
    ) -> Option<(StateIndex, u8)> {
        let compiled = self.resident();
        let role = compiled.role();
        if arm >= 2 {
            return None;
        }
        let (start, end) = self.resident_route_arm_bounds(compiled, scope_id, arm)?;
        let view = compiled.program_image().view();
        let mut idx = start;
        while idx < end && idx < view.len() {
            let node = view.node_at(idx);
            if matches!(node.kind, EffKind::Atom) {
                let atom = node.atom_data();
                if atom.from == role {
                    let step = self.resident_step_for_eff(role, compiled, idx)?;
                    return Some((StateIndex::from_usize(step), atom.label));
                }
            }
            idx += 1;
        }
        None
    }

    #[inline(always)]
    pub(crate) fn route_recv_state(&self, scope_id: ScopeId, arm: u8) -> Option<StateIndex> {
        let compiled = self.resident();
        let role = compiled.role();
        if arm >= 2 {
            return None;
        }
        let (start, end) = self.resident_route_arm_bounds(compiled, scope_id, arm)?;
        let view = compiled.program_image().view();
        let mut idx = start;
        while idx < end && idx < view.len() {
            let node = view.node_at(idx);
            if matches!(node.kind, EffKind::Atom) {
                let atom = node.atom_data();
                if atom.to == role && atom.from != role {
                    let step = self.resident_step_for_eff(role, compiled, idx)?;
                    return Some(StateIndex::from_usize(step));
                }
            }
            idx += 1;
        }
        None
    }

    #[inline(always)]
    pub(crate) fn passive_arm_entry(&self, scope_id: ScopeId, arm: u8) -> Option<StateIndex> {
        let compiled = self.resident();
        let role = compiled.role();
        if arm >= 2 {
            return None;
        }
        let (start, end) = self.resident_route_arm_bounds(compiled, scope_id, arm)?;
        let view = compiled.program_image().view();
        let mut idx = start;
        while idx < end && idx < view.len() {
            let node = view.node_at(idx);
            if matches!(node.kind, EffKind::Atom) {
                let atom = node.atom_data();
                if atom.from == role || atom.to == role {
                    let step = self.resident_step_for_eff(role, compiled, idx)?;
                    return Some(StateIndex::from_usize(step));
                }
            }
            idx += 1;
        }
        None
    }

    #[inline(always)]
    pub(crate) fn first_recv_dispatch_table(
        &self,
        scope_id: ScopeId,
    ) -> Option<([FirstRecvDispatchSpec; MAX_FIRST_RECV_DISPATCH], u8)> {
        let compiled = self.resident();
        let role = compiled.role();
        let mut table = [FirstRecvDispatchSpec::EMPTY; MAX_FIRST_RECV_DISPATCH];
        let mut len = 0usize;
        let view = compiled.program_image().view();
        let mut arm = 0u8;
        while arm < 2 && len < table.len() {
            if let Some((start, end)) = self.resident_route_arm_bounds(compiled, scope_id, arm) {
                let mut idx = start;
                while idx < end && idx < view.len() {
                    let node = view.node_at(idx);
                    if matches!(node.kind, EffKind::Atom) {
                        let atom = node.atom_data();
                        if atom.to == role && atom.from != role {
                            let Some(step) = self.resident_step_for_eff(role, compiled, idx) else {
                                idx += 1;
                                continue;
                            };
                            table[len] = FirstRecvDispatchSpec::new(
                                self.resident_frame_label_at(compiled, idx),
                                atom.lane,
                                arm,
                                StateIndex::from_usize(step),
                            );
                            len += 1;
                            break;
                        }
                    }
                    idx += 1;
                }
            }
            arm += 1;
        }
        Some((table, len as u8))
    }
    #[inline(always)]
    pub(crate) fn first_recv_dispatch_target_for_lane_frame_label(
        &self,
        scope_id: ScopeId,
        lane: u8,
        frame_label: u8,
    ) -> Option<(u8, StateIndex)> {
        let (table, len) = self.first_recv_dispatch_table(scope_id)?;
        let mut idx = 0usize;
        let mut matched = None;
        while idx < len as usize {
            let entry = table[idx];
            if entry.frame_label() == frame_label && entry.lane() == lane {
                if let Some((arm, _)) = matched
                    && arm != entry.arm()
                {
                    return None;
                }
                matched = Some((entry.arm(), entry.target()));
            }
            idx += 1;
        }
        matched
    }

    #[inline(always)]
    pub(crate) fn route_scope_dense_ordinal(&self, scope_id: ScopeId) -> Option<usize> {
        let compiled = self.resident();
        self.resident_route_scope_dense_ordinal(compiled, scope_id)
    }
}
