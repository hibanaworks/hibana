use crate::global::const_dsl::{EffList, ResolverMode, ScopeEvent, ScopeId, ScopeKind};
use crate::global::steps::RoleLaneMask;

use super::ProgramSourceError;

pub(crate) trait ProgramTerm {
    const PROGRAM_SOURCE: ProgramSourceData;
}

#[derive(Clone, Copy)]
pub(crate) struct ProgramSourceData {
    eff: EffList,
    role_lane_mask: RoleLaneMask,
    lane_span: u16,
    error: Option<ProgramSourceError>,
}

#[derive(Clone, Copy)]
pub(crate) struct RouteHead {
    pub(crate) controller: u8,
    pub(crate) label: u8,
    pub(crate) error: Option<ProgramSourceError>,
}

impl ProgramSourceData {
    pub(crate) const fn from_parts(
        eff: EffList,
        role_lane_mask: RoleLaneMask,
        lane_span: u16,
    ) -> Self {
        Self {
            eff,
            role_lane_mask,
            lane_span,
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
                error: Some(ProgramSourceError::RouteArmHead),
            };
        }
        let node = self.eff.node_at(0);
        if !matches!(node.kind, crate::eff::EffKind::Atom) {
            return RouteHead {
                controller: 0,
                label: 0,
                error: Some(ProgramSourceError::RouteArmHead),
            };
        }
        let atom = node.atom_data();
        RouteHead {
            controller: atom.from,
            label: atom.label,
            error: None,
        }
    }

    pub(crate) const fn seq(self, next: Self) -> Self {
        let error = Self::merge_error(self.error, next.error);
        let rebased = next.eff.rebase_scopes(self.scope_budget());
        let eff = self.eff.extend_list(rebased);
        add_scope_budget(self.scope_budget(), next.scope_budget());
        Self {
            eff,
            role_lane_mask: self.role_lane_mask.union(next.role_lane_mask),
            lane_span: max_lane_span(self.lane_span, next.lane_span),
            error,
        }
    }

    pub(crate) const fn resolve_route(self, resolver_id: u16) -> Self {
        let mut error = self.error;
        if resolver_id == u16::MAX {
            error = Self::merge_error(error, Some(ProgramSourceError::ResolverIdReserved));
        }
        let eff = if self.eff.is_empty() {
            error = Self::merge_error(error, Some(ProgramSourceError::ResolverTargetNotRoute));
            self.eff
        } else {
            let scope = ScopeId::route(0);
            let mut eff = self.eff;
            let mut found = false;
            let markers = self.eff.scope_markers();
            let mut marker_idx = 0usize;
            while marker_idx < markers.len() {
                let marker = markers[marker_idx];
                if matches!(marker.event, ScopeEvent::Enter)
                    && matches!(marker.scope_kind, ScopeKind::Route)
                    && marker.scope_id.canonical().raw() == scope.canonical().raw()
                {
                    eff = eff.push_resolver(
                        marker.offset,
                        ResolverMode::dynamic(resolver_id).with_scope(scope),
                    );
                    found = true;
                }
                marker_idx += 1;
            }
            if !found {
                error = Self::merge_error(error, Some(ProgramSourceError::ResolverTargetNotRoute));
            }
            eff
        };
        Self {
            eff,
            role_lane_mask: self.role_lane_mask,
            lane_span: self.lane_span,
            error,
        }
    }

    pub(crate) const fn roll(self) -> Self {
        let mut error = self.error;
        if self.eff.is_empty() {
            error = Self::merge_error(error, Some(ProgramSourceError::LoopBodyEmpty));
        }
        let loop_scope = ScopeId::loop_scope(0);
        let eff = self
            .into_eff()
            .rebase_scopes(1)
            .mark_route_scopes_linger()
            .with_scope(loop_scope);
        Self {
            eff,
            role_lane_mask: self.role_lane_mask,
            lane_span: self.lane_span,
            error,
        }
    }

    pub(crate) const fn route_with_controller(
        self,
        right: Self,
        controller: u8,
        route_error: Option<ProgramSourceError>,
    ) -> Self {
        let mut error = Self::merge_error(self.error, right.error);
        error = Self::merge_error(error, route_error);
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
        Self {
            eff: left_eff.extend_list(right_eff),
            role_lane_mask: self.role_lane_mask.union(right.role_lane_mask),
            lane_span: max_lane_span(self.lane_span, right.lane_span),
            error,
        }
    }

    pub(crate) const fn par(self, right: Self) -> Self {
        let mut error = Self::merge_error(self.error, right.error);
        if self.eff.is_empty() || right.eff.is_empty() {
            error = Self::merge_error(error, Some(ProgramSourceError::ParallelEmpty));
        }
        let right_role_lane_mask = right.role_lane_mask.shift_lanes(self.lane_span);
        if self.role_lane_mask.intersects(&right_role_lane_mask) {
            error = Self::merge_error(error, Some(ProgramSourceError::ParallelConflict));
        }
        let parallel_scope = ScopeId::parallel(0);
        let left_budget = self.scope_budget();
        let right_offset = add_scope_budget(1, left_budget);
        let left_eff = self.into_eff().rebase_scopes(1);
        let right_eff = right
            .into_eff()
            .rebase_lanes(self.lane_span)
            .rebase_scopes(right_offset);
        Self {
            eff: left_eff.extend_list(right_eff).with_scope(parallel_scope),
            role_lane_mask: self.role_lane_mask.union(right_role_lane_mask),
            lane_span: add_lane_span(self.lane_span, right.lane_span),
            error,
        }
    }
}

const fn max_lane_span(lhs: u16, rhs: u16) -> u16 {
    if lhs >= rhs { lhs } else { rhs }
}

const fn add_lane_span(lhs: u16, rhs: u16) -> u16 {
    let sum = lhs as u32 + rhs as u32;
    if sum > (u8::MAX as u32 + 1) {
        panic!("projection internal lane overflow");
    }
    sum as u16
}

const fn add_scope_budget(lhs: u16, rhs: u16) -> u16 {
    let sum = lhs as u32 + rhs as u32;
    if sum > ScopeId::ORDINAL_CAPACITY as u32 {
        panic!("structured scope budget exceeded");
    }
    sum as u16
}
