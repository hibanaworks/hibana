use crate::global::LoopControlMeaning;
use crate::global::const_dsl::{EffList, PolicyMode, ScopeId, ScopeKind};
use crate::global::steps::RoleLaneMask;

use super::ProgramSourceError;

pub(crate) trait ProgramTerm {
    const PROGRAM_SOURCE: ProgramSourceData;
}

#[derive(Clone, Copy)]
pub(crate) struct ProgramSourceData {
    eff: EffList,
    role_lane_mask: RoleLaneMask,
    cycle_scope_pending: bool,
    tail_is_cycle_control: bool,
    error: Option<ProgramSourceError>,
}

#[derive(Clone, Copy)]
pub(crate) struct RouteHead {
    pub(crate) controller: u8,
    pub(crate) label: u8,
    pub(crate) cycle_meaning: Option<LoopControlMeaning>,
    pub(crate) error: Option<ProgramSourceError>,
}

impl ProgramSourceData {
    pub(crate) const fn from_parts(
        eff: EffList,
        role_lane_mask: RoleLaneMask,
        cycle_scope_pending: bool,
        tail_is_cycle_control: bool,
    ) -> Self {
        Self {
            eff,
            role_lane_mask,
            cycle_scope_pending,
            tail_is_cycle_control,
            error: None,
        }
    }

    pub(crate) const fn merge_error(
        left: Option<ProgramSourceError>,
        right: Option<ProgramSourceError>,
    ) -> Option<ProgramSourceError> {
        if left.is_some() { left } else { right }
    }

    #[inline(always)]
    pub(crate) const fn eff_list(&self) -> &EffList {
        &self.eff
    }

    #[inline(always)]
    pub(crate) const fn error(&self) -> Option<ProgramSourceError> {
        self.error
    }

    #[inline(always)]
    const fn scope_budget(&self) -> u16 {
        self.eff.scope_budget()
    }

    #[inline(always)]
    const fn into_eff(self) -> EffList {
        self.eff
    }

    pub(crate) const fn route_head(&self) -> RouteHead {
        if self.eff.is_empty() {
            return RouteHead {
                controller: 0,
                label: 0,
                cycle_meaning: None,
                error: Some(ProgramSourceError::RouteArmHead),
            };
        }
        let node = self.eff.node_at(0);
        if !matches!(node.kind, crate::eff::EffKind::Atom) {
            return RouteHead {
                controller: 0,
                label: 0,
                cycle_meaning: None,
                error: Some(ProgramSourceError::RouteArmHead),
            };
        }
        let atom = node.atom_data();
        if atom.from != atom.to {
            return RouteHead {
                controller: atom.from,
                label: atom.label,
                cycle_meaning: LoopControlMeaning::from_control_spec(self.eff.control_spec_at(0)),
                error: Some(ProgramSourceError::RouteArmHead),
            };
        }
        RouteHead {
            controller: atom.from,
            label: atom.label,
            cycle_meaning: LoopControlMeaning::from_control_spec(self.eff.control_spec_at(0)),
            error: None,
        }
    }

    pub(crate) const fn seq(self, next: Self) -> Self {
        let mut error = Self::merge_error(self.error, next.error);
        let next_tail_is_cycle_control = if next.eff.is_empty() {
            self.tail_is_cycle_control
        } else {
            next.tail_is_cycle_control
        };
        let rebased = next.eff.rebase_scopes(self.scope_budget());
        let mut eff = self.eff;
        let scope_budget = self.scope_budget();
        if next.cycle_scope_pending {
            if eff.is_empty() {
                error = Self::merge_error(error, Some(ProgramSourceError::LoopBodyEmpty));
                eff = eff.extend_list(rebased);
            } else {
                let cycle_scope = ScopeId::new(
                    ScopeKind::Loop,
                    add_scope_budget(scope_budget, next.scope_budget()),
                );
                let scoped_next = rebased.with_scope(cycle_scope);
                eff = if self.tail_is_cycle_control {
                    eff.with_scope(cycle_scope).extend_list(scoped_next)
                } else {
                    eff.extend_list(scoped_next)
                };
                add_scope_budget(scope_budget, add_scope_budget(next.scope_budget(), 1));
            }
        } else {
            eff = eff.extend_list(rebased);
            add_scope_budget(scope_budget, next.scope_budget());
        }
        Self {
            eff,
            role_lane_mask: self.role_lane_mask.union(next.role_lane_mask),
            cycle_scope_pending: false,
            tail_is_cycle_control: next_tail_is_cycle_control,
            error,
        }
    }

    pub(crate) const fn with_policy(self, policy_id: u16) -> Self {
        let mut error = self.error;
        if policy_id == crate::global::ControlDesc::STATIC_POLICY_SITE {
            error = Self::merge_error(error, Some(ProgramSourceError::PolicyIdReserved));
        }
        let eff = if self.eff.is_empty() {
            error = Self::merge_error(error, Some(ProgramSourceError::PolicyRequiresControlHead));
            self.eff
        } else {
            self.eff.with_policy(PolicyMode::dynamic(policy_id))
        };
        Self {
            eff,
            role_lane_mask: self.role_lane_mask,
            cycle_scope_pending: self.cycle_scope_pending,
            tail_is_cycle_control: self.tail_is_cycle_control,
            error,
        }
    }

    pub(crate) const fn route_with_controller(
        self,
        right: Self,
        controller: u8,
        is_cycle: bool,
        route_error: Option<ProgramSourceError>,
    ) -> Self {
        let mut error = Self::merge_error(self.error, right.error);
        error = Self::merge_error(error, route_error);
        error = Self::merge_error(
            error,
            ProgramSourceError::from_policy_head_status(
                self.eff.route_arm_dynamic_policy_head_status(),
            ),
        );
        error = Self::merge_error(
            error,
            ProgramSourceError::from_policy_head_status(
                right.eff.route_arm_dynamic_policy_head_status(),
            ),
        );
        let scope = ScopeId::route(0);
        let left_budget = self.scope_budget();
        let left_arm = self.into_eff();
        let right_arm = right.into_eff();
        let right_offset = add_scope_budget(1, left_budget);
        let left_eff = left_arm
            .rebase_scopes(1)
            .with_scope_controller(scope, controller);
        let right_eff = right_arm
            .rebase_scopes(right_offset)
            .with_scope(scope)
            .with_scope_controller_role(scope, controller);
        let eff = left_eff.extend_list(right_eff);
        let eff = if is_cycle {
            eff.with_scope_linger(scope, true)
        } else {
            eff
        };
        let cycle_scope_pending = eff.scope_has_linger(scope);
        Self {
            eff,
            role_lane_mask: self.role_lane_mask.union(right.role_lane_mask),
            cycle_scope_pending,
            tail_is_cycle_control: right.tail_is_cycle_control,
            error,
        }
    }

    pub(crate) const fn par(self, right: Self) -> Self {
        let mut error = Self::merge_error(self.error, right.error);
        if self.eff.is_empty() || right.eff.is_empty() {
            error = Self::merge_error(error, Some(ProgramSourceError::ParallelEmpty));
        }
        if self.role_lane_mask.intersects(&right.role_lane_mask) {
            error = Self::merge_error(error, Some(ProgramSourceError::ParallelConflict));
        }
        let parallel_scope = ScopeId::parallel(0);
        let left_budget = self.scope_budget();
        let right_offset = add_scope_budget(1, left_budget);
        let left_eff = self.into_eff().rebase_scopes(1);
        let right_eff = right.into_eff().rebase_scopes(right_offset);
        Self {
            eff: left_eff.extend_list(right_eff).with_scope(parallel_scope),
            role_lane_mask: self.role_lane_mask.union(right.role_lane_mask),
            cycle_scope_pending: false,
            tail_is_cycle_control: right.tail_is_cycle_control,
            error,
        }
    }
}

const fn add_scope_budget(lhs: u16, rhs: u16) -> u16 {
    let sum = lhs as u32 + rhs as u32;
    if sum > ScopeId::ORDINAL_CAPACITY as u32 {
        panic!("structured scope budget exceeded");
    }
    sum as u16
}
