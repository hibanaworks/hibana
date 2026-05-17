use crate::{
    control::cluster::effects::{EffectEnvelopeRef, ProgramImageDynamicPolicySiteIter},
    eff::{EffIndex, EffKind},
    endpoint::kernel::EndpointArenaLayout,
    global::const_dsl::{PolicyMode, ScopeEvent, ScopeId, ScopeKind},
    global::typestate::{LocalNode, MAX_FIRST_RECV_DISPATCH, ScopeRegion, StateIndex},
};

use super::{
    program::{ControlSemanticKind, ControlSemanticsTable, DynamicPolicySite},
    role::{CompiledRoleImage, PhaseLaneEntry},
};
use crate::global::{
    compiled::lowering::{CompiledProgramImage, ProgramStamp},
    role_program::{DENSE_LANE_NONE, DenseLaneOrdinal, LaneSetView, LaneSteps, PhaseRouteGuard},
};

#[inline(always)]
fn same_scope(left: ScopeId, right: ScopeId) -> bool {
    !left.is_none() && left.canonical_raw() == right.canonical_raw()
}

#[inline(always)]
fn first_enter_for_scope(
    markers: &[crate::global::const_dsl::ScopeMarker],
    marker_idx: usize,
) -> bool {
    let marker = markers[marker_idx];
    if !matches!(marker.event, ScopeEvent::Enter) {
        return false;
    }
    let mut idx = 0usize;
    while idx < marker_idx {
        let candidate = markers[idx];
        if matches!(candidate.event, ScopeEvent::Enter)
            && same_scope(candidate.scope_id, marker.scope_id)
        {
            return false;
        }
        idx += 1;
    }
    true
}

/// Sealed runtime owner for immutable program-wide compiled facts.
#[derive(Clone, Copy)]
pub(crate) struct CompiledProgramRef {
    stamp: ProgramStamp,
    image: &'static CompiledProgramImage,
}

impl core::fmt::Debug for CompiledProgramRef {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("CompiledProgramRef")
            .field("stamp", &self.stamp.words())
            .field("image", &(self.image as *const CompiledProgramImage))
            .finish()
    }
}

impl PartialEq for CompiledProgramRef {
    #[inline(always)]
    fn eq(&self, other: &Self) -> bool {
        self.stamp.words() == other.stamp.words() && core::ptr::eq(self.image, other.image)
    }
}

impl Eq for CompiledProgramRef {}

impl CompiledProgramRef {
    #[inline(always)]
    pub(crate) const fn resident(
        stamp: ProgramStamp,
        image: &'static CompiledProgramImage,
    ) -> Self {
        Self { stamp, image }
    }

    #[inline(always)]
    pub(crate) fn effect_envelope(&self) -> EffectEnvelopeRef<'_> {
        EffectEnvelopeRef::from_program_image(self.image)
    }

    #[inline(always)]
    pub(crate) fn role_count(&self) -> usize {
        self.image.compiled_program_role_count()
    }

    #[inline(always)]
    pub(crate) fn dynamic_policy_sites_for(
        &self,
        policy_id: u16,
    ) -> impl Iterator<Item = DynamicPolicySite> + '_ {
        ProgramImageDynamicPolicySiteIter::new(self.image)
            .filter(move |site| site.policy_id() == policy_id)
    }

    #[inline(always)]
    pub(crate) fn control_semantics(&self) -> &'static ControlSemanticsTable {
        &super::program::CONTROL_SEMANTICS_TABLE
    }

    pub(crate) fn validate_label_universe(
        &self,
        max: u8,
    ) -> Result<(), crate::global::role_program::LabelUniverseViolation> {
        if max == u8::MAX {
            return Ok(());
        }

        let view = self.image.view();
        let mut idx = 0usize;
        while idx < view.len() {
            let node = view.node_at(idx);
            if matches!(node.kind, crate::eff::EffKind::Atom) {
                let actual = node.atom_data().label;
                if actual > max {
                    return Err(crate::global::role_program::LabelUniverseViolation {
                        max,
                        actual,
                    });
                }
            }
            idx += 1;
        }

        Ok(())
    }

    #[inline(always)]
    pub(crate) fn route_controller_role(&self, scope_id: ScopeId) -> Option<u8> {
        crate::global::compiled::lowering::program_lowering::compiled_program_route_control_for_scope(
            self.image,
            scope_id,
        )
        .and_then(|record| record.controller_role())
    }

    #[inline(always)]
    pub(crate) fn route_controller(
        &self,
        scope_id: ScopeId,
    ) -> Option<(
        PolicyMode,
        crate::eff::EffIndex,
        u8,
        crate::control::cap::mint::ControlOp,
    )> {
        crate::global::compiled::lowering::program_lowering::compiled_program_route_control_for_scope(
            self.image,
            scope_id,
        )
        .and_then(|record| record.route_controller())
    }
}

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
    pub(crate) fn phase_lane_set(&self, idx: usize) -> Option<LaneSetView> {
        let compiled = self.resident();
        let role = compiled.role();
        if idx != 0 {
            return None;
        }
        let mut lanes = LaneSetView::from_lane_count(self.logical_lane_count());
        let view = compiled.program_image().view();
        let mut eff_idx = 0usize;
        while eff_idx < view.len() {
            let node = view.node_at(eff_idx);
            if matches!(node.kind, EffKind::Atom) {
                let atom = node.atom_data();
                if atom.from == role || atom.to == role {
                    lanes.insert(atom.lane as usize);
                }
            }
            eff_idx += 1;
        }
        Some(lanes)
    }

    #[inline(always)]
    pub(crate) fn phase_min_start(&self, idx: usize) -> Option<u16> {
        (idx == 0 && self.local_len() != 0).then_some(0)
    }

    #[inline(always)]
    pub(crate) fn phase_route_guard(&self, idx: usize) -> Option<PhaseRouteGuard> {
        let _ = idx;
        None
    }

    #[inline(always)]
    pub(crate) fn phase_lane_steps(&self, idx: usize, lane_idx: usize) -> Option<LaneSteps> {
        let compiled = self.resident();
        let role = compiled.role();
        if idx != 0 || lane_idx >= self.logical_lane_count() {
            return None;
        }
        let view = compiled.program_image().view();
        let mut step = 0usize;
        let mut start = usize::MAX;
        let mut len = 0usize;
        let mut eff_idx = 0usize;
        while eff_idx < view.len() {
            let node = view.node_at(eff_idx);
            if matches!(node.kind, EffKind::Atom) {
                let atom = node.atom_data();
                if atom.from == role || atom.to == role {
                    if atom.lane as usize == lane_idx {
                        if start == usize::MAX {
                            start = step;
                        }
                        len += 1;
                    }
                    step += 1;
                }
            }
            eff_idx += 1;
        }
        if len == 0 {
            None
        } else {
            Some(LaneSteps {
                start: start as u16,
                len: len as u16,
            })
        }
    }

    #[inline(always)]
    pub(crate) fn phase_lane_entries(&self, idx: usize) -> &[PhaseLaneEntry] {
        let _ = idx;
        &[]
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
        let enclosing_loop = self.resident_enclosing_loop_scope_at_offset(compiled, eff_idx);
        let next = StateIndex::from_usize(action_ordinal.saturating_add(1));
        let eff_index = EffIndex::from_dense_ordinal(eff_idx);
        if atom.from == role && atom.to == role {
            LocalNode::local(
                eff_index,
                atom.label,
                frame_label,
                resource,
                atom.is_control,
                shot,
                policy,
                atom.lane,
                semantic,
                next,
                scope,
                enclosing_loop,
                route_arm,
                false,
            )
        } else if atom.from == role {
            LocalNode::send(
                eff_index,
                atom.to,
                atom.label,
                frame_label,
                resource,
                atom.is_control,
                shot,
                policy,
                atom.lane,
                semantic,
                next,
                scope,
                enclosing_loop,
                route_arm,
                false,
            )
        } else {
            LocalNode::recv(
                eff_index,
                atom.from,
                atom.label,
                frame_label,
                resource,
                atom.is_control,
                shot,
                policy,
                atom.lane,
                semantic,
                next,
                scope,
                enclosing_loop,
                route_arm,
                route_scope_and_arm.is_some_and(|(route_scope, arm)| {
                    self.resident_first_recv_eff_for_route_arm(role, compiled, route_scope, arm)
                        == Some(eff_idx)
                }),
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

    fn resident_first_step_at_or_after(
        &self,
        role: u8,
        compiled: &CompiledRoleImage,
        start_eff: usize,
    ) -> usize {
        let view = compiled.program_image().view();
        let mut step = 0usize;
        let mut idx = 0usize;
        while idx < view.len() {
            let node = view.node_at(idx);
            if matches!(node.kind, EffKind::Atom) {
                let atom = node.atom_data();
                if atom.from == role || atom.to == role {
                    if idx >= start_eff {
                        return step;
                    }
                    step += 1;
                }
            }
            idx += 1;
        }
        self.local_len()
    }

    fn resident_frame_label_at(&self, compiled: &CompiledRoleImage, offset: usize) -> u8 {
        let view = compiled.program_image().view();
        let current = view.node_at(offset).atom_data();
        let mut frame = 0usize;
        let mut idx = 0usize;
        while idx < offset {
            let candidate = view.node_at(idx);
            if matches!(candidate.kind, EffKind::Atom) {
                let atom = candidate.atom_data();
                if atom.to == current.to && atom.lane == current.lane {
                    frame += 1;
                }
            }
            idx += 1;
        }
        if frame > u8::MAX as usize {
            panic!("frame label allocation overflow");
        }
        frame as u8
    }

    fn resident_scope_at(&self, compiled: &CompiledRoleImage, eff_idx: usize) -> ScopeId {
        let view = compiled.program_image().view();
        let markers = view.scope_markers();
        let mut best = ScopeId::none();
        let mut best_start = 0usize;
        let mut best_span = usize::MAX;
        let mut idx = 0usize;
        while idx < markers.len() {
            let marker = markers[idx];
            if matches!(marker.event, ScopeEvent::Enter)
                && self.resident_scope_segment_contains(markers, idx, eff_idx)
            {
                let start = marker.offset;
                if best.is_none() || start > best_start {
                    best = marker.scope_id;
                    best_start = start;
                    best_span = usize::MAX;
                } else if start == best_start {
                    let end = self.resident_scope_segment_end(
                        markers,
                        idx,
                        compiled.program_image().view().len(),
                    );
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

    fn resident_scope_slot(&self, compiled: &CompiledRoleImage, scope_id: ScopeId) -> Option<u16> {
        if scope_id.is_none() {
            return None;
        }
        let view = compiled.program_image().view();
        let markers = view.scope_markers();
        let mut slot = 0u16;
        let mut idx = 0usize;
        while idx < markers.len() {
            let marker = markers[idx];
            if first_enter_for_scope(markers, idx) {
                if same_scope(marker.scope_id, scope_id) {
                    return Some(slot);
                }
                slot = slot.saturating_add(1);
            }
            idx += 1;
        }
        None
    }

    fn resident_route_scope_dense_ordinal(
        &self,
        compiled: &CompiledRoleImage,
        scope_id: ScopeId,
    ) -> Option<usize> {
        if scope_id.is_none() {
            return None;
        }
        let markers = compiled.program_image().view().scope_markers();
        let mut dense = 0usize;
        let mut idx = 0usize;
        while idx < markers.len() {
            let marker = markers[idx];
            if first_enter_for_scope(markers, idx) && matches!(marker.scope_kind, ScopeKind::Route)
            {
                if same_scope(marker.scope_id, scope_id) {
                    return Some(dense);
                }
                dense = dense.saturating_add(1);
            }
            idx += 1;
        }
        None
    }

    fn resident_route_scope_by_dense_ordinal(
        &self,
        compiled: &CompiledRoleImage,
        dense_target: usize,
    ) -> Option<ScopeId> {
        let markers = compiled.program_image().view().scope_markers();
        let mut dense = 0usize;
        let mut idx = 0usize;
        while idx < markers.len() {
            let marker = markers[idx];
            if first_enter_for_scope(markers, idx) && matches!(marker.scope_kind, ScopeKind::Route)
            {
                if dense == dense_target {
                    return Some(marker.scope_id);
                }
                dense = dense.saturating_add(1);
            }
            idx += 1;
        }
        None
    }

    fn resident_route_scope_by_scope_slot(
        &self,
        compiled: &CompiledRoleImage,
        slot_target: usize,
    ) -> Option<ScopeId> {
        self.resident_route_scope_by_dense_ordinal(compiled, slot_target)
    }

    fn resident_first_scope_segment_bounds(
        &self,
        compiled: &CompiledRoleImage,
        scope_id: ScopeId,
    ) -> Option<(ScopeKind, usize, usize, bool)> {
        if scope_id.is_none() {
            return None;
        }
        let markers = compiled.program_image().view().scope_markers();
        let default_end = compiled.program_image().view().len();
        let mut idx = 0usize;
        while idx < markers.len() {
            let marker = markers[idx];
            if first_enter_for_scope(markers, idx) && same_scope(marker.scope_id, scope_id) {
                return Some((
                    marker.scope_kind,
                    marker.offset,
                    self.resident_scope_segment_end(markers, idx, default_end),
                    marker.linger,
                ));
            }
            idx += 1;
        }
        None
    }

    fn resident_scope_segment_end(
        &self,
        markers: &[crate::global::const_dsl::ScopeMarker],
        enter_idx: usize,
        default_end: usize,
    ) -> usize {
        let marker = markers[enter_idx];
        let mut scan = enter_idx + 1;
        while scan < markers.len() {
            let candidate = markers[scan];
            if same_scope(candidate.scope_id, marker.scope_id)
                && matches!(candidate.event, ScopeEvent::Exit)
            {
                return candidate.offset;
            }
            scan += 1;
        }
        default_end
    }

    fn resident_scope_segment_contains(
        &self,
        markers: &[crate::global::const_dsl::ScopeMarker],
        enter_idx: usize,
        eff_idx: usize,
    ) -> bool {
        let marker = markers[enter_idx];
        if marker.offset > eff_idx {
            return false;
        }
        let mut scan = enter_idx + 1;
        while scan < markers.len() {
            let candidate = markers[scan];
            if candidate.offset > eff_idx {
                return true;
            }
            if same_scope(candidate.scope_id, marker.scope_id)
                && matches!(candidate.event, ScopeEvent::Exit)
            {
                return false;
            }
            scan += 1;
        }
        true
    }

    fn resident_route_arm_ranges(
        &self,
        markers: &[crate::global::const_dsl::ScopeMarker],
        route: ScopeId,
    ) -> Option<[(usize, usize); 2]> {
        if route.is_none() {
            return None;
        }
        let mut starts = [usize::MAX; 2];
        let mut ends = [usize::MAX; 2];
        let mut enter_len = 0usize;
        let mut exit_len = 0usize;
        let mut idx = 0usize;
        while idx < markers.len() {
            let marker = markers[idx];
            if same_scope(marker.scope_id, route) && marker.scope_kind == ScopeKind::Route {
                match marker.event {
                    ScopeEvent::Enter => {
                        if enter_len < 2 {
                            starts[enter_len] = marker.offset;
                        }
                        enter_len += 1;
                    }
                    ScopeEvent::Exit => {
                        if exit_len < 2 {
                            ends[exit_len] = marker.offset;
                        }
                        exit_len += 1;
                    }
                }
            }
            idx += 1;
        }
        (enter_len == 2 && exit_len == 2).then_some([(starts[0], ends[0]), (starts[1], ends[1])])
    }

    fn resident_scope_bounds(
        &self,
        compiled: &CompiledRoleImage,
        scope_id: ScopeId,
    ) -> Option<(ScopeKind, usize, usize, bool, u16)> {
        if scope_id.is_none() {
            return None;
        }
        let markers = compiled.program_image().view().scope_markers();
        let default_end = compiled.program_image().view().len();
        let mut idx = 0usize;
        let mut kind = ScopeKind::Generic;
        let mut start = usize::MAX;
        let mut end = 0usize;
        let mut linger = false;
        let mut found = false;
        while idx < markers.len() {
            let marker = markers[idx];
            if same_scope(marker.scope_id, scope_id) && matches!(marker.event, ScopeEvent::Enter) {
                found = true;
                kind = marker.scope_kind;
                if marker.scope_kind == ScopeKind::Route
                    && let Some(ranges) = self.resident_route_arm_ranges(markers, scope_id)
                {
                    let route_start = core::cmp::min(ranges[0].0, ranges[1].0);
                    let route_end = core::cmp::max(ranges[0].1, ranges[1].1);
                    if route_start < start {
                        start = route_start;
                    }
                    if route_end > end {
                        end = route_end;
                    }
                    linger |= marker.linger;
                    break;
                }
                if marker.offset < start {
                    start = marker.offset;
                }
                linger |= marker.linger;
                let segment_end = self.resident_scope_segment_end(markers, idx, default_end);
                if segment_end > end {
                    end = segment_end;
                }
            }
            idx += 1;
        }
        if found {
            let range = self.resident_scope_slot(compiled, scope_id).unwrap_or(0);
            Some((kind, start, end, linger, range))
        } else {
            None
        }
    }

    fn resident_parent_scope(
        &self,
        compiled: &CompiledRoleImage,
        scope_id: ScopeId,
    ) -> Option<ScopeId> {
        let (_, target_start, target_end, _) =
            self.resident_first_scope_segment_bounds(compiled, scope_id)?;
        let markers = compiled.program_image().view().scope_markers();
        let default_end = compiled.program_image().view().len();
        let mut best = ScopeId::none();
        let mut best_span = usize::MAX;
        let mut best_start = 0usize;
        let mut idx = 0usize;
        while idx < markers.len() {
            let marker = markers[idx];
            if matches!(marker.event, ScopeEvent::Enter)
                && !same_scope(marker.scope_id, scope_id)
                && {
                    let start = marker.offset;
                    let end = self.resident_scope_segment_end(markers, idx, default_end);
                    start <= target_start && target_end <= end
                }
            {
                let start = marker.offset;
                let end = self.resident_scope_segment_end(markers, idx, default_end);
                let span = end.saturating_sub(start);
                if best.is_none() || span < best_span || (span == best_span && start > best_start) {
                    best = marker.scope_id;
                    best_span = span;
                    best_start = start;
                }
            }
            idx += 1;
        }
        if best.is_none() { None } else { Some(best) }
    }

    fn resident_ancestor_scope_of_kind(
        &self,
        compiled: &CompiledRoleImage,
        scope_id: ScopeId,
        kind: ScopeKind,
    ) -> Option<ScopeId> {
        let mut current = Some(scope_id);
        let mut depth = 0usize;
        let bound = compiled
            .program_image()
            .view()
            .scope_markers()
            .len()
            .saturating_add(1);
        while let Some(scope) = current {
            if let Some((scope_kind, _, _, _, _)) = self.resident_scope_bounds(compiled, scope)
                && scope_kind == kind
            {
                return Some(scope);
            }
            if depth >= bound {
                return None;
            }
            depth += 1;
            current = self.resident_parent_scope(compiled, scope);
        }
        None
    }

    fn resident_enclosing_loop_scope_at_offset(
        &self,
        compiled: &CompiledRoleImage,
        eff_idx: usize,
    ) -> Option<ScopeId> {
        let view = compiled.program_image().view();
        let markers = view.scope_markers();
        let default_end = view.len();
        let mut best = ScopeId::none();
        let mut best_start = 0usize;
        let mut best_span = usize::MAX;
        let mut idx = 0usize;
        while idx < markers.len() {
            let marker = markers[idx];
            if matches!(marker.event, ScopeEvent::Enter)
                && (marker.scope_kind == ScopeKind::Loop
                    || (marker.scope_kind == ScopeKind::Route && marker.linger))
            {
                let start = marker.offset;
                let end = self.resident_scope_segment_end(markers, idx, default_end);
                if start <= eff_idx && eff_idx < end {
                    let span = end.saturating_sub(start);
                    if best.is_none()
                        || start > best_start
                        || (start == best_start && span < best_span)
                    {
                        best = marker.scope_id;
                        best_start = start;
                        best_span = span;
                    }
                }
            }
            idx += 1;
        }
        if best.is_none() { None } else { Some(best) }
    }

    fn resident_enclosing_loop_scope(
        &self,
        compiled: &CompiledRoleImage,
        scope_id: ScopeId,
    ) -> Option<ScopeId> {
        let mut current = Some(scope_id);
        let mut depth = 0usize;
        let bound = compiled
            .program_image()
            .view()
            .scope_markers()
            .len()
            .saturating_add(1);
        while let Some(scope) = current {
            if let Some((scope_kind, _, _, linger, _)) = self.resident_scope_bounds(compiled, scope)
                && (scope_kind == ScopeKind::Loop || (scope_kind == ScopeKind::Route && linger))
            {
                return Some(scope);
            }
            if depth >= bound {
                return None;
            }
            depth += 1;
            current = self.resident_parent_scope(compiled, scope);
        }
        None
    }

    fn resident_route_scope_and_arm_at(
        &self,
        compiled: &CompiledRoleImage,
        eff_idx: usize,
    ) -> Option<(ScopeId, u8)> {
        let route = self.resident_route_scope_at_offset(compiled, eff_idx)?;
        let arm = self.resident_route_arm_for_scope_offset(compiled, route, eff_idx)?;
        Some((route, arm))
    }

    fn resident_route_scope_at_offset(
        &self,
        compiled: &CompiledRoleImage,
        eff_idx: usize,
    ) -> Option<ScopeId> {
        let view = compiled.program_image().view();
        let markers = view.scope_markers();
        let mut best = ScopeId::none();
        let mut best_start = 0usize;
        let mut best_span = usize::MAX;
        let mut idx = 0usize;
        while idx < markers.len() {
            let marker = markers[idx];
            if matches!(marker.event, ScopeEvent::Enter)
                && matches!(marker.scope_kind, ScopeKind::Route)
                && self.resident_scope_segment_contains(markers, idx, eff_idx)
            {
                let start = marker.offset;
                let span = if best.is_none() || start == best_start {
                    self.resident_scope_segment_end(markers, idx, view.len())
                        .saturating_sub(start)
                } else {
                    0
                };
                if best.is_none() || start > best_start || (start == best_start && span < best_span)
                {
                    best = marker.scope_id;
                    best_start = start;
                    best_span = span;
                }
            }
            idx += 1;
        }
        if best.is_none() { None } else { Some(best) }
    }

    fn resident_route_arm_for_scope_offset(
        &self,
        compiled: &CompiledRoleImage,
        route: ScopeId,
        eff_idx: usize,
    ) -> Option<u8> {
        let markers = compiled.program_image().view().scope_markers();
        let ranges = self.resident_route_arm_ranges(markers, route)?;
        if ranges[0].0 <= eff_idx && eff_idx < ranges[0].1 {
            return Some(0);
        }
        if ranges[1].0 <= eff_idx && eff_idx < ranges[1].1 {
            return Some(1);
        }
        None
    }

    fn resident_offset_belongs_to_route_arm(
        &self,
        compiled: &CompiledRoleImage,
        route: ScopeId,
        arm: u8,
        eff_idx: usize,
    ) -> bool {
        let mut current = self.resident_scope_at(compiled, eff_idx);
        if current.is_none() {
            return false;
        }
        if same_scope(current, route) {
            return self.resident_route_arm_for_scope_offset(compiled, route, eff_idx) == Some(arm);
        }
        let bound = compiled
            .program_image()
            .view()
            .scope_markers()
            .len()
            .saturating_add(1);
        let mut depth = 0usize;
        while !current.is_none() && !same_scope(current, route) && depth < bound {
            if current.kind() != ScopeKind::Route {
                let Some(parent) = self.resident_parent_scope(compiled, current) else {
                    return false;
                };
                current = parent;
                depth += 1;
                continue;
            }
            let Some(parent) = self.route_parent(current) else {
                return false;
            };
            if same_scope(parent, route) {
                return self.route_parent_arm(current) == Some(arm);
            }
            current = parent;
            depth += 1;
        }
        false
    }

    fn resident_route_arm_bounds(
        &self,
        compiled: &CompiledRoleImage,
        route: ScopeId,
        target_arm: u8,
    ) -> Option<(usize, usize)> {
        if target_arm >= 2 {
            return None;
        }
        let view = compiled.program_image().view();
        let markers = view.scope_markers();
        self.resident_route_arm_ranges(markers, route)
            .map(|ranges| ranges[target_arm as usize])
    }

    fn resident_first_recv_eff_for_route_arm(
        &self,
        role: u8,
        compiled: &CompiledRoleImage,
        route: ScopeId,
        arm: u8,
    ) -> Option<usize> {
        let (start, end) = self.resident_route_arm_bounds(compiled, route, arm)?;
        let view = compiled.program_image().view();
        let mut idx = start;
        while idx < end && idx < view.len() {
            let node = view.node_at(idx);
            if matches!(node.kind, EffKind::Atom) {
                let atom = node.atom_data();
                if atom.to == role && atom.from != role {
                    return Some(idx);
                }
            }
            idx += 1;
        }
        None
    }

    #[inline(always)]
    pub(crate) fn step_for_eff_index(&self, eff_index: EffIndex) -> Option<usize> {
        let compiled = self.resident();
        let role = compiled.role();
        self.resident_step_for_eff(role, compiled, eff_index.dense_ordinal())
    }

    #[inline(always)]
    pub(crate) fn state_for_step_index(&self, step_idx: usize) -> Option<StateIndex> {
        if step_idx < self.local_len() {
            Some(StateIndex::from_usize(step_idx))
        } else {
            None
        }
    }

    #[inline(always)]
    pub(crate) fn scope_region_by_id(&self, scope_id: ScopeId) -> Option<ScopeRegion> {
        let compiled = self.resident();
        let role = compiled.role();
        let (kind, start_eff, end_eff, linger, range) =
            self.resident_scope_bounds(compiled, scope_id)?;
        let start = self.resident_first_step_at_or_after(role, compiled, start_eff);
        let end = self.resident_first_step_at_or_after(role, compiled, end_eff);
        Some(ScopeRegion {
            scope_id,
            kind,
            start,
            end,
            range,
            nest: 0,
            linger,
            controller_role: self.program().route_controller_role(scope_id),
        })
    }

    #[inline(always)]
    pub(crate) fn scope_parent(&self, scope_id: ScopeId) -> Option<ScopeId> {
        let compiled = self.resident();
        self.resident_parent_scope(compiled, scope_id)
    }

    #[inline(always)]
    pub(crate) fn control_parent(&self, scope_id: ScopeId) -> Option<ScopeId> {
        self.scope_parent(scope_id)
    }

    #[inline(always)]
    pub(crate) fn route_parent(&self, scope_id: ScopeId) -> Option<ScopeId> {
        let compiled = self.resident();
        let parent = self.resident_parent_scope(compiled, scope_id)?;
        self.resident_ancestor_scope_of_kind(compiled, parent, ScopeKind::Route)
    }

    #[inline(always)]
    pub(crate) fn route_parent_arm(&self, scope_id: ScopeId) -> Option<u8> {
        let compiled = self.resident();
        let parent = self.resident_parent_scope(compiled, scope_id)?;
        let (_, start, _, _, _) = self.resident_scope_bounds(compiled, scope_id)?;
        let parent_route =
            self.resident_ancestor_scope_of_kind(compiled, parent, ScopeKind::Route)?;
        self.resident_route_arm_for_scope_offset(compiled, parent_route, start)
    }

    #[inline(always)]
    pub(crate) fn parallel_root(&self, scope_id: ScopeId) -> Option<ScopeId> {
        let compiled = self.resident();
        self.resident_ancestor_scope_of_kind(compiled, scope_id, ScopeKind::Parallel)
    }

    #[inline(always)]
    pub(crate) fn enclosing_loop(&self, scope_id: ScopeId) -> Option<ScopeId> {
        let compiled = self.resident();
        self.resident_enclosing_loop_scope(compiled, scope_id)
    }

    #[inline(always)]
    pub(crate) fn frontier_scratch_layout(&self) -> crate::endpoint::kernel::FrontierScratchLayout {
        crate::endpoint::kernel::FrontierScratchLayout::new(
            self.max_frontier_entries(),
            self.logical_lane_count(),
            crate::global::role_program::lane_word_count(self.logical_lane_count()),
        )
    }

    #[inline(always)]
    pub(crate) fn max_frontier_entries(&self) -> usize {
        self.footprint().frontier_entry_count
    }

    #[inline(always)]
    pub(crate) fn route_scope_arm_lane_set_by_slot(
        &self,
        slot: usize,
        arm: u8,
    ) -> Option<LaneSetView> {
        let compiled = self.resident();
        let scope = self.resident_route_scope_by_scope_slot(compiled, slot)?;
        let (_, start, end, _, _) = self.resident_scope_bounds(compiled, scope)?;
        let mut lanes = LaneSetView::from_lane_count(self.logical_lane_count());
        let view = compiled.program_image().view();
        let mut idx = start;
        while idx < end && idx < view.len() {
            let node = view.node_at(idx);
            if matches!(node.kind, EffKind::Atom)
                && self.resident_offset_belongs_to_route_arm(compiled, scope, arm, idx)
            {
                let atom = node.atom_data();
                lanes.insert(atom.lane as usize);
            }
            idx += 1;
        }
        Some(lanes)
    }

    #[inline(always)]
    pub(crate) fn route_scope_offer_lane_set_by_slot(&self, slot: usize) -> Option<LaneSetView> {
        let mut lanes = LaneSetView::from_lane_count(self.logical_lane_count());
        let mut arm = 0u8;
        while arm < 2 {
            let arm_lanes = self.route_scope_arm_lane_set_by_slot(slot, arm)?;
            let mut lane_idx = 0usize;
            while lane_idx < self.logical_lane_count() {
                if arm_lanes.contains(lane_idx) {
                    lanes.insert(lane_idx);
                }
                lane_idx += 1;
            }
            arm += 1;
        }
        Some(lanes)
    }

    #[inline(always)]
    pub(crate) fn route_scope_offer_entry_by_slot(&self, slot: usize) -> Option<StateIndex> {
        let compiled = self.resident();
        let scope = self.resident_route_scope_by_scope_slot(compiled, slot)?;
        let (_, start, _, _, _) = self.resident_scope_bounds(compiled, scope)?;
        let step = self.resident_first_step_at_or_after(self.role(), compiled, start);
        if step < self.local_len() {
            Some(StateIndex::from_usize(step))
        } else {
            Some(StateIndex::MAX)
        }
    }

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
    ) -> Option<([(u8, u8, u8, StateIndex); MAX_FIRST_RECV_DISPATCH], u8)> {
        let compiled = self.resident();
        let role = compiled.role();
        let mut table = [(0, 0, 0, StateIndex::MAX); MAX_FIRST_RECV_DISPATCH];
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
                            table[len] = (
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
    pub(crate) fn first_recv_dispatch_frame_label_mask(
        &self,
        scope_id: ScopeId,
    ) -> crate::transport::FrameLabelMask {
        let Some((table, len)) = self.first_recv_dispatch_table(scope_id) else {
            return crate::transport::FrameLabelMask::EMPTY;
        };
        let mut mask = crate::transport::FrameLabelMask::EMPTY;
        let mut idx = 0usize;
        while idx < len as usize {
            mask.insert_frame_label(table[idx].0);
            idx += 1;
        }
        mask
    }

    #[inline(always)]
    pub(crate) fn first_recv_dispatch_arm_mask(&self, scope_id: ScopeId) -> u8 {
        let Some((table, len)) = self.first_recv_dispatch_table(scope_id) else {
            return 0;
        };
        let mut mask = 0u8;
        let mut idx = 0usize;
        while idx < len as usize {
            mask |= 1 << table[idx].2;
            idx += 1;
        }
        mask
    }

    #[inline(always)]
    pub(crate) fn first_recv_dispatch_lane_mask(&self, scope_id: ScopeId, arm: u8) -> u8 {
        let Some((table, len)) = self.first_recv_dispatch_table(scope_id) else {
            return 0;
        };
        let mut mask = 0u8;
        let mut idx = 0usize;
        while idx < len as usize {
            if table[idx].2 == arm && table[idx].1 < 8 {
                mask |= 1 << table[idx].1;
            }
            idx += 1;
        }
        mask
    }

    #[inline(always)]
    pub(crate) fn first_recv_dispatch_arm_frame_label_mask(
        &self,
        scope_id: ScopeId,
        arm: u8,
    ) -> crate::transport::FrameLabelMask {
        let Some((table, len)) = self.first_recv_dispatch_table(scope_id) else {
            return crate::transport::FrameLabelMask::EMPTY;
        };
        let mut mask = crate::transport::FrameLabelMask::EMPTY;
        let mut idx = 0usize;
        while idx < len as usize {
            if table[idx].2 == arm {
                mask.insert_frame_label(table[idx].0);
            }
            idx += 1;
        }
        mask
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
        while idx < len as usize {
            let (entry_frame, entry_lane, arm, target) = table[idx];
            if entry_frame == frame_label && entry_lane == lane {
                return Some((arm, target));
            }
            idx += 1;
        }
        None
    }

    #[inline(always)]
    pub(crate) fn route_scope_dense_ordinal(&self, scope_id: ScopeId) -> Option<usize> {
        let compiled = self.resident();
        self.resident_route_scope_dense_ordinal(compiled, scope_id)
    }

    #[inline(always)]
    pub(crate) fn has_active_lane(&self, lane_idx: usize) -> bool {
        let mut dense = [DENSE_LANE_NONE; crate::global::role_program::LANE_DOMAIN_SIZE];
        let count = self.fill_active_lane_dense_by_lane(&mut dense);
        lane_idx < dense.len() && count != 0 && dense[lane_idx] != DENSE_LANE_NONE
    }

    #[inline(always)]
    pub(crate) fn first_active_lane(&self) -> Option<usize> {
        let mut dense = [DENSE_LANE_NONE; crate::global::role_program::LANE_DOMAIN_SIZE];
        let count = self.fill_active_lane_dense_by_lane(&mut dense);
        if count == 0 {
            return None;
        }
        dense.iter().position(|lane| *lane != DENSE_LANE_NONE)
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
        let lane_slots = self.route_table_lane_slots();
        if lane_slots == 0 {
            0
        } else {
            lane_slots.saturating_mul(self.max_route_stack_depth().max(1))
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
        let compiled = self.resident();
        let role = compiled.role();
        dst.fill(DENSE_LANE_NONE);
        let view = compiled.program_image().view();
        let mut idx = 0usize;
        while idx < view.len() {
            let node = view.node_at(idx);
            if matches!(node.kind, crate::eff::EffKind::Atom) {
                let atom = node.atom_data();
                if atom.from == role || atom.to == role {
                    let lane = atom.lane as usize;
                    if lane < dst.len() {
                        dst[lane] = DenseLaneOrdinal::ZERO;
                    }
                }
            }
            idx += 1;
        }
        let mut lane_idx = 0usize;
        let mut dense = 0usize;
        while lane_idx < dst.len() {
            if dst[lane_idx] != DENSE_LANE_NONE {
                dst[lane_idx] =
                    DenseLaneOrdinal::new(dense).expect("dense active lane ordinal fits u16");
                dense += 1;
            }
            lane_idx += 1;
        }
        dense
    }

    #[inline(always)]
    pub(crate) fn fill_logical_lane_dense_by_lane(&self, dst: &mut [DenseLaneOrdinal]) -> usize {
        let logical_lane_count = self.logical_lane_count();
        let mut lane_idx = 0usize;
        while lane_idx < dst.len() {
            dst[lane_idx] = if lane_idx < logical_lane_count {
                DenseLaneOrdinal::new(lane_idx).expect("logical lane ordinal fits u16")
            } else {
                DENSE_LANE_NONE
            };
            lane_idx += 1;
        }
        core::cmp::min(logical_lane_count, dst.len())
    }

    #[inline(always)]
    pub(crate) fn endpoint_arena_layout_for_binding(
        &self,
        binding_enabled: bool,
    ) -> EndpointArenaLayout {
        EndpointArenaLayout::from_footprint_with_binding(
            self.endpoint_layout_footprint(),
            binding_enabled,
        )
    }
}

/// Sealed runtime owner for role-local immutable compiled facts within a compiled program ref.
#[derive(Clone, Copy)]
pub(crate) struct RoleImageSlice<const ROLE: u8> {
    descriptor: RoleDescriptorRef,
}

impl<const ROLE: u8> RoleImageSlice<ROLE> {
    #[inline(always)]
    pub(crate) const fn from_resident(compiled: &'static CompiledRoleImage) -> Self {
        Self {
            descriptor: RoleDescriptorRef::from_resident(compiled),
        }
    }

    #[inline(always)]
    pub(crate) const fn descriptor(&self) -> RoleDescriptorRef {
        self.descriptor
    }

    #[inline(always)]
    pub(crate) const fn program(&self) -> CompiledProgramRef {
        self.descriptor.program()
    }

    #[inline(always)]
    pub(crate) fn has_active_lane(&self, lane_idx: usize) -> bool {
        self.descriptor.has_active_lane(lane_idx)
    }

    #[inline(always)]
    pub(crate) fn first_active_lane(&self) -> Option<usize> {
        self.descriptor.first_active_lane()
    }

    #[inline(always)]
    pub(crate) fn endpoint_lane_slot_count(&self) -> usize {
        self.descriptor.endpoint_lane_slot_count()
    }

    #[inline(always)]
    pub(crate) fn logical_lane_count(&self) -> usize {
        self.descriptor.logical_lane_count()
    }

    #[inline(always)]
    pub(crate) fn route_table_frame_slots(&self) -> usize {
        self.descriptor.route_table_frame_slots()
    }

    #[inline(always)]
    pub(crate) fn route_table_lane_slots(&self) -> usize {
        self.descriptor.route_table_lane_slots()
    }

    #[inline(always)]
    pub(crate) fn loop_table_slots(&self) -> usize {
        self.descriptor.loop_table_slots()
    }

    #[inline(always)]
    pub(crate) fn resident_cap_entries(&self) -> usize {
        self.descriptor.resident_cap_entries()
    }

    #[cfg(test)]
    #[inline(always)]
    pub(crate) fn active_lane_count(&self) -> usize {
        self.descriptor.active_lane_count()
    }

    #[cfg(test)]
    #[inline(always)]
    pub(crate) fn max_route_stack_depth(&self) -> usize {
        self.descriptor.max_route_stack_depth()
    }

    #[cfg(test)]
    #[inline(always)]
    pub(crate) fn max_loop_stack_depth(&self) -> usize {
        self.descriptor.max_loop_stack_depth()
    }

    #[cfg(test)]
    #[inline(always)]
    pub(crate) fn route_scope_count(&self) -> usize {
        self.descriptor.route_scope_count()
    }

    #[inline(always)]
    pub(crate) fn endpoint_arena_layout_for_binding(
        &self,
        binding_enabled: bool,
    ) -> EndpointArenaLayout {
        self.descriptor
            .endpoint_arena_layout_for_binding(binding_enabled)
    }
}
