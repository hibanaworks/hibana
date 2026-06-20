use crate::global::const_dsl::{
    EffList, INTRINSIC_ROUTE_RESOLVER_ID, ScopeEvent, ScopeId, ScopeKind, ScopeRebase,
};
use crate::global::steps::RoleLaneMask;

use super::ProgramSourceError;

pub(crate) trait ProgramTerm {
    const PROGRAM_SOURCE: ProgramSourceData;
}

pub(crate) struct ProgramSourceData {
    eff: EffList,
    role_lane_mask: RoleLaneMask,
    lane_span: u16,
    error: Option<ProgramSourceError>,
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

    pub(crate) const fn seq(left: Self, next: Self) -> Self {
        let error = Self::merge_error(left.error, next.error);
        let left_budget = left.eff.scope_budget();
        let right_budget = next.eff.scope_budget();
        let mut eff = left.eff;
        eff.append_rebased_from(&next.eff, 0, left_budget, ScopeRebase::Preserve);
        add_scope_budget(left_budget, right_budget);
        let lane_span = max_lane_span(left.lane_span, next.lane_span);
        Self {
            eff,
            role_lane_mask: left.role_lane_mask.union(next.role_lane_mask, lane_span),
            lane_span,
            error,
        }
    }

    pub(crate) const fn resolve_route(source: Self, resolver_id: u16) -> Self {
        let mut eff = source.eff;
        let mut error = source.error;
        if resolver_id == INTRINSIC_ROUTE_RESOLVER_ID {
            error = Self::merge_error(error, Some(ProgramSourceError::ResolverIdOutOfDomain));
        }
        if eff.is_empty() {
            error = Self::merge_error(error, Some(ProgramSourceError::ResolverTargetNotRoute));
        } else {
            let scope = ScopeId::route(0);
            let mut found = false;
            let mut marker_idx = 0usize;
            while marker_idx < eff.scope_marker_count() {
                let marker = eff.scope_marker_at(marker_idx);
                if matches!(marker.event, ScopeEvent::Enter)
                    && matches!(marker.scope_id.kind(), Some(ScopeKind::Route))
                    && marker.scope_id.same(scope)
                    && !found
                {
                    eff.push_route_resolver_mut(scope, resolver_id);
                    found = true;
                }
                marker_idx += 1;
            }
            if !found {
                error = Self::merge_error(error, Some(ProgramSourceError::ResolverTargetNotRoute));
            }
        }
        Self {
            eff,
            role_lane_mask: source.role_lane_mask,
            lane_span: source.lane_span,
            error,
        }
    }

    pub(crate) const fn roll(source: Self) -> Self {
        let mut error = source.error;
        if source.eff.is_empty() {
            error = Self::merge_error(error, Some(ProgramSourceError::RollBodyAbsent));
        }
        let roll_scope = ScopeId::roll_scope(0);
        let mut eff = source.eff;
        eff.rebase_scopes_mut(1, ScopeRebase::MarkRouteEnters);
        let len = eff.len();
        eff.push_scope_around(0, len, roll_scope);
        Self {
            eff,
            role_lane_mask: source.role_lane_mask,
            lane_span: source.lane_span,
            error,
        }
    }

    pub(crate) const fn route(left: Self, right: Self) -> Self {
        let mut error = Self::merge_error(left.error, right.error);
        if left.eff.is_empty() || right.eff.is_empty() {
            error = Self::merge_error(error, Some(ProgramSourceError::RouteArmAbsent));
        }
        let scope = ScopeId::route(0);
        let left_budget = left.eff.scope_budget();
        let right_offset = add_scope_budget(1, left_budget);
        let mut eff = left.eff;
        eff.rebase_scopes_mut(1, ScopeRebase::Preserve);
        let left_start = 0usize;
        let left_end = eff.len();
        eff.push_scope_around(left_start, left_end, scope);
        let right_start = eff.len();
        eff.push_scope_enter_at_boundary(right_start, scope);
        eff.append_rebased_from(&right.eff, 0, right_offset, ScopeRebase::Preserve);
        let right_end = eff.len();
        eff.push_scope_exit_at_boundary(right_end, scope);
        let lane_span = max_lane_span(left.lane_span, right.lane_span);
        Self {
            eff,
            role_lane_mask: left.role_lane_mask.union(right.role_lane_mask, lane_span),
            lane_span,
            error,
        }
    }

    pub(crate) const fn par(left: Self, right: Self) -> Self {
        let mut error = Self::merge_error(left.error, right.error);
        if left.eff.is_empty() || right.eff.is_empty() {
            error = Self::merge_error(error, Some(ProgramSourceError::ParallelArmAbsent));
        }
        let right_role_lane_mask = right
            .role_lane_mask
            .shift_lanes(left.lane_span, right.lane_span);
        let combined_lane_span = add_lane_span(left.lane_span, right.lane_span);
        if left
            .role_lane_mask
            .intersects(&right_role_lane_mask, combined_lane_span)
        {
            error = Self::merge_error(error, Some(ProgramSourceError::ParallelConflict));
        }
        let parallel_scope = ScopeId::parallel(0);
        let left_budget = left.eff.scope_budget();
        let right_offset = add_scope_budget(1, left_budget);
        let mut eff = left.eff;
        eff.rebase_scopes_mut(1, ScopeRebase::Preserve);
        let left_len = eff.len();
        eff.append_rebased_from(
            &right.eff,
            left.lane_span,
            right_offset,
            ScopeRebase::Preserve,
        );
        eff.push_parallel_scope_split(parallel_scope, left_len);
        Self {
            eff,
            role_lane_mask: left
                .role_lane_mask
                .union(right_role_lane_mask, combined_lane_span),
            lane_span: combined_lane_span,
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
    if sum > ScopeId::LOCAL_CAPACITY as u32 {
        panic!("structured scope budget exceeded");
    }
    sum as u16
}
