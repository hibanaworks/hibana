use super::{
    CompiledRoleImage, EffKind, LaneSetView, LocalDependency, PackedEventConflict,
    RoleDescriptorRef, ScopeEvent, ScopeId, ScopeKind, ScopeRegion, StateIndex,
    first_enter_for_scope, same_scope,
};
mod dispatch;

impl RoleDescriptorRef {
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

    pub(super) fn resident_frame_label_at(
        &self,
        compiled: &CompiledRoleImage,
        offset: usize,
    ) -> u8 {
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

    pub(super) fn resident_scope_at(
        &self,
        compiled: &CompiledRoleImage,
        eff_idx: usize,
    ) -> ScopeId {
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

    pub(super) fn resident_route_scope_and_arm_at(
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

    pub(super) fn resident_first_recv_eff_for_route_arm(
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
    pub(crate) fn route_scope_linger(&self, scope_id: ScopeId) -> bool {
        self.resident_scope_bounds(self.resident(), scope_id)
            .map(|(kind, _, _, linger, _)| kind == ScopeKind::Route && linger)
            .unwrap_or(false)
    }

    #[inline(always)]
    pub(crate) fn scope_parent(&self, scope_id: ScopeId) -> Option<ScopeId> {
        let compiled = self.resident();
        self.resident_parent_scope(compiled, scope_id)
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

    #[inline]
    pub(crate) fn route_ancestor_arm(&self, scope_id: ScopeId, ancestor: ScopeId) -> Option<u8> {
        if scope_id.is_none() || ancestor.is_none() || scope_id == ancestor {
            return None;
        }
        let mut current = scope_id;
        let mut depth = 0usize;
        let depth_bound = self.route_scope_count().saturating_add(1);
        while depth < depth_bound {
            let parent = self.route_parent(current)?;
            if parent == current {
                return None;
            }
            let arm = self.route_parent_arm(current)?;
            if parent == ancestor {
                return Some(arm);
            }
            current = parent;
            depth += 1;
        }
        None
    }

    #[inline]
    pub(crate) fn route_scope_for_selected_child_arm(
        &self,
        scope_id: ScopeId,
        arm: u8,
    ) -> Option<ScopeId> {
        if scope_id.kind() == ScopeKind::Route {
            return Some(scope_id);
        }
        let route_scope = self.route_parent(scope_id)?;
        (self.route_parent_arm(scope_id) == Some(arm)).then_some(route_scope)
    }

    #[inline(always)]
    pub(crate) fn parallel_root(&self, scope_id: ScopeId) -> Option<ScopeId> {
        let compiled = self.resident();
        self.resident_ancestor_scope_of_kind(compiled, scope_id, ScopeKind::Parallel)
    }

    #[inline(always)]
    pub(crate) fn dependency_for_index(&self, current_idx: usize) -> Option<LocalDependency> {
        self.resident()
            .role_image()
            .dependency_for_index(current_idx)
    }

    #[inline(always)]
    pub(crate) fn event_conflict_for_index(&self, current_idx: usize) -> PackedEventConflict {
        self.resident()
            .role_image()
            .event_conflict_for_index(current_idx)
    }

    #[inline(always)]
    pub(crate) fn route_scope_conflict_by_slot(&self, slot: usize) -> PackedEventConflict {
        self.resident()
            .role_image()
            .route_scope_conflict_by_slot(slot)
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
    ) -> Option<LaneSetView<'static>> {
        self.resident()
            .role_image()
            .route_scope_arm_lane_set_by_slot(slot, arm)
    }

    #[inline(always)]
    pub(crate) fn route_scope_offer_lane_set_by_slot(
        &self,
        slot: usize,
    ) -> Option<LaneSetView<'static>> {
        self.resident()
            .role_image()
            .route_scope_offer_lane_set_by_slot(slot)
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
}
