use super::{CompiledRoleImage, EffKind, RoleDescriptorRef, ScopeId, ScopeKind, same_scope};
use crate::global::const_dsl::ScopeEvent;

impl RoleDescriptorRef {
    pub(super) fn resident_frame_label_at(
        &self,
        compiled: &CompiledRoleImage,
        offset: usize,
    ) -> u8 {
        let view = compiled.role_image().program_image().view();
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
            panic!("frame label universe overflow");
        }
        frame as u8
    }

    pub(super) fn resident_scope_at(
        &self,
        compiled: &CompiledRoleImage,
        eff_idx: usize,
    ) -> ScopeId {
        let view = compiled.role_image().program_image().view();
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
                        compiled.role_image().program_image().view().len(),
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
        let view = compiled.role_image().program_image().view();
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
        let markers = compiled.role_image().program_image().view().scope_markers();
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
        let view = compiled.role_image().program_image().view();
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
        let view = compiled.role_image().program_image().view();
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
}
