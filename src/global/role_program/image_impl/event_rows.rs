use super::super::{
    CompiledProgramImage, MAX_LOCAL_STEP_LANES, RoleLaneImage, ScopeEvent, ScopeId,
};
use crate::global::{
    compiled::images::ControlSemanticKind,
    typestate::{LocalAtomFacts, LocalNode, LocalNodeMeta, StateIndex},
};

impl RoleLaneImage {
    #[inline(always)]
    pub(crate) const fn local_step_node(&self, step_idx: usize) -> Option<LocalNode> {
        if step_idx >= MAX_LOCAL_STEP_LANES {
            None
        } else {
            Some(self.local_step_nodes[step_idx])
        }
    }

    #[inline(always)]
    const fn scope_at(program: &CompiledProgramImage, eff_idx: usize) -> ScopeId {
        let view = program.view();
        let markers = view.scope_markers();
        let mut best = ScopeId::none();
        let mut best_start = 0usize;
        let mut best_span = usize::MAX;
        let mut idx = 0usize;
        while idx < markers.len() {
            let marker = markers[idx];
            if marker.offset > eff_idx {
                break;
            }
            if matches!(marker.event, ScopeEvent::Enter) {
                let start = marker.offset;
                let end = Self::scope_segment_end(markers, idx, view.len());
                if eff_idx >= end {
                    idx += 1;
                    continue;
                }
                if best.is_none() || start > best_start {
                    best = marker.scope_id;
                    best_start = start;
                    best_span = usize::MAX;
                } else if start == best_start {
                    let span = end.saturating_sub(start);
                    if span < best_span {
                        best = marker.scope_id;
                        best_start = start;
                        best_span = span;
                    }
                }
            }
            idx += 1;
        }
        best
    }

    #[inline(always)]
    const fn route_scope_and_arm_at(
        program: &CompiledProgramImage,
        eff_idx: usize,
    ) -> Option<(ScopeId, u8)> {
        match Self::route_conflict_for_eff(program.view().scope_markers(), eff_idx).to_conflict() {
            Some(crate::global::typestate::LocalConflict::RouteArm { scope, arm }) => {
                Some((scope, arm))
            }
            Some(_) | None => None,
        }
    }

    #[inline(always)]
    const fn first_recv_eff_for_route_arm<const ROLE: u8>(
        program: &CompiledProgramImage,
        route: ScopeId,
        arm: u8,
    ) -> Option<usize> {
        if arm >= 2 {
            return None;
        }
        let markers = program.view().scope_markers();
        let Some(ranges) = Self::route_arm_ranges(markers, route) else {
            return None;
        };
        let (start, end) = ranges[arm as usize];
        let view = program.view();
        let mut idx = start;
        while idx < end && idx < view.len() {
            if let Some(atom) = view.atom_at(idx) {
                if atom.to == ROLE && atom.from != ROLE {
                    return Some(idx);
                }
            }
            idx += 1;
        }
        None
    }

    #[inline(always)]
    pub(super) const fn local_node_for_eff<const ROLE: u8>(
        program: &CompiledProgramImage,
        eff_idx: usize,
        action_ordinal: usize,
        frame_label: u8,
    ) -> LocalNode {
        let view = program.view();
        let atom = view.node_at(eff_idx).atom_data();
        let scope = Self::scope_at(program, eff_idx);
        let policy = match view.resident_policy_at(eff_idx) {
            Some(policy) => policy.with_scope(scope),
            None => crate::global::const_dsl::PolicyMode::Static,
        };
        let control_desc = if atom.is_control {
            view.resident_control_desc_at(eff_idx)
        } else {
            None
        };
        let semantic = ControlSemanticKind::from_control_desc(control_desc);
        let shot = match control_desc {
            Some(desc) => Some(desc.shot()),
            None => None,
        };
        let route_scope_and_arm = Self::route_scope_and_arm_at(program, eff_idx);
        let route_arm = match route_scope_and_arm {
            Some((_, arm)) => Some(arm),
            None => None,
        };
        let next = StateIndex::from_usize(action_ordinal.saturating_add(1));
        let eff_index = crate::eff::EffIndex::from_dense_ordinal(eff_idx);
        let facts = LocalAtomFacts {
            eff_index,
            label: atom.label,
            frame_label,
            resource: atom.resource,
            is_control: atom.is_control,
            shot,
            policy,
            lane: atom.lane,
        };
        if atom.from == ROLE && atom.to == ROLE {
            LocalNode::local(
                facts,
                LocalNodeMeta {
                    semantic,
                    next,
                    scope,
                    route_arm,
                    is_choice_determinant: false,
                },
            )
        } else if atom.from == ROLE {
            LocalNode::send(
                atom.to,
                facts,
                LocalNodeMeta {
                    semantic,
                    next,
                    scope,
                    route_arm,
                    is_choice_determinant: false,
                },
            )
        } else {
            let choice = match route_scope_and_arm {
                Some((route_scope, arm)) => {
                    match Self::first_recv_eff_for_route_arm::<ROLE>(program, route_scope, arm) {
                        Some(first) => first == eff_idx,
                        None => false,
                    }
                }
                None => false,
            };
            LocalNode::recv(
                atom.from,
                facts,
                LocalNodeMeta {
                    semantic,
                    next,
                    scope,
                    route_arm,
                    is_choice_determinant: choice,
                },
            )
        }
    }
}
