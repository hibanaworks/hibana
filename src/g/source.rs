use crate::global::const_dsl::{
    EffList, INTRINSIC_ROUTE_RESOLVER_ID, RouteFrontierSummary, ScopeEvent, ScopeId, ScopeKind,
};
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
    frontier: SourceFrontier,
    error: Option<ProgramSourceError>,
}

#[derive(Clone, Copy)]
struct SourceFrontier {
    label_words: [u64; 4],
    controller_mask: u16,
    empty: bool,
    invalid: bool,
}

impl SourceFrontier {
    const EMPTY: Self = Self {
        label_words: [0; 4],
        controller_mask: 0,
        empty: true,
        invalid: false,
    };

    const fn from_eff(eff: &EffList) -> Self {
        if eff.is_empty() {
            return Self::EMPTY;
        }
        let node = eff.node_at(0);
        if !matches!(node.kind, crate::eff::EffKind::Atom) {
            return Self::EMPTY;
        }
        let atom = node.atom_data();
        Self::atom(atom.from, atom.label)
    }

    const fn atom(from: u8, label: u8) -> Self {
        let mut words = [0u64; 4];
        let word = (label / 64) as usize;
        let bit = label % 64;
        words[word] = 1u64 << bit;
        let invalid = from >= u16::BITS as u8;
        Self {
            label_words: words,
            controller_mask: if invalid { 0 } else { 1u16 << from },
            empty: false,
            invalid,
        }
    }

    const fn seq(self, next: Self) -> Self {
        if self.empty { next } else { self }
    }

    const fn union(self, other: Self) -> Self {
        let mut words = [0u64; 4];
        let mut idx = 0usize;
        while idx < words.len() {
            words[idx] = self.label_words[idx] | other.label_words[idx];
            idx += 1;
        }
        Self {
            label_words: words,
            controller_mask: self.controller_mask | other.controller_mask,
            empty: self.empty && other.empty,
            invalid: self.invalid || other.invalid,
        }
    }

    const fn label_collision(self, other: Self) -> bool {
        let mut idx = 0usize;
        while idx < self.label_words.len() {
            if (self.label_words[idx] & other.label_words[idx]) != 0 {
                return true;
            }
            idx += 1;
        }
        false
    }

    const fn route_summary(scope: ScopeId, left: Self, right: Self) -> RouteFrontierSummary {
        RouteFrontierSummary::new(
            scope,
            left.controller_mask | right.controller_mask,
            left.empty || right.empty || left.invalid || right.invalid,
            left.label_collision(right),
        )
    }
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
            frontier: SourceFrontier::from_eff(&eff),
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

    pub(crate) const fn seq(self, next: Self) -> Self {
        let error = Self::merge_error(self.error, next.error);
        let rebased = next.eff.rebase_scopes(self.scope_budget());
        let eff = self.eff.extend_list(rebased);
        add_scope_budget(self.scope_budget(), next.scope_budget());
        Self {
            eff,
            role_lane_mask: self.role_lane_mask.union(
                next.role_lane_mask,
                max_lane_span(self.lane_span, next.lane_span),
            ),
            lane_span: max_lane_span(self.lane_span, next.lane_span),
            frontier: self.frontier.seq(next.frontier),
            error,
        }
    }

    pub(crate) const fn resolve_route(self, resolver_id: u16) -> Self {
        let mut error = self.error;
        if resolver_id == INTRINSIC_ROUTE_RESOLVER_ID {
            error = Self::merge_error(error, Some(ProgramSourceError::ResolverIdOutOfDomain));
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
                    && marker.scope_id.same(scope)
                    && !found
                {
                    eff = eff.push_route_resolver(scope, resolver_id);
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
            frontier: self.frontier,
            error,
        }
    }

    pub(crate) const fn roll(self) -> Self {
        let mut error = self.error;
        if self.eff.is_empty() {
            error = Self::merge_error(error, Some(ProgramSourceError::RollBodyAbsent));
        }
        let roll_scope = ScopeId::roll_scope(0);
        let eff = self
            .into_eff()
            .rebase_scopes(1)
            .mark_route_scopes_reentry()
            .with_scope(roll_scope);
        Self {
            eff,
            role_lane_mask: self.role_lane_mask,
            lane_span: self.lane_span,
            frontier: self.frontier,
            error,
        }
    }

    pub(crate) const fn route(self, right: Self) -> Self {
        let mut error = Self::merge_error(self.error, right.error);
        if self.eff.is_empty() || right.eff.is_empty() {
            error = Self::merge_error(error, Some(ProgramSourceError::RouteArmAbsent));
        }
        let scope = ScopeId::route(0);
        let left_budget = self.scope_budget();
        let left_arm = self.into_eff();
        let right_arm = right.into_eff();
        let right_offset = add_scope_budget(1, left_budget);
        let left_eff = left_arm.rebase_scopes(1).with_scope(scope);
        let right_eff = right_arm.rebase_scopes(right_offset).with_scope(scope);
        let route_summary = SourceFrontier::route_summary(scope, self.frontier, right.frontier);
        Self {
            eff: left_eff
                .extend_list(right_eff)
                .push_route_frontier(route_summary),
            role_lane_mask: self.role_lane_mask.union(
                right.role_lane_mask,
                max_lane_span(self.lane_span, right.lane_span),
            ),
            lane_span: max_lane_span(self.lane_span, right.lane_span),
            frontier: self.frontier.union(right.frontier),
            error,
        }
    }

    pub(crate) const fn par(self, right: Self) -> Self {
        let mut error = Self::merge_error(self.error, right.error);
        if self.eff.is_empty() || right.eff.is_empty() {
            error = Self::merge_error(error, Some(ProgramSourceError::ParallelArmAbsent));
        }
        let right_role_lane_mask = right
            .role_lane_mask
            .shift_lanes(self.lane_span, right.lane_span);
        let combined_lane_span = add_lane_span(self.lane_span, right.lane_span);
        if self
            .role_lane_mask
            .intersects(&right_role_lane_mask, combined_lane_span)
        {
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
            role_lane_mask: self
                .role_lane_mask
                .union(right_role_lane_mask, combined_lane_span),
            lane_span: combined_lane_span,
            frontier: self.frontier.union(right.frontier),
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
