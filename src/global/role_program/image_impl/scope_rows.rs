use super::super::{
    MAX_ROUTE_ARM_LANE_ROWS, PackedLaneRange, PackedRouteArmRow, RoleLaneScratch, ScopeEvent,
    ScopeId, ScopeKind, ScopeMarker,
};
use crate::global::typestate::{LocalConflict, PackedEventConflict};

#[derive(Clone, Copy)]
pub(super) struct RouteArmProjectionRowInput<'a> {
    pub(super) markers: &'a [ScopeMarker],
    pub(super) view_len: usize,
    pub(super) route_slot: usize,
    pub(super) route_scope: ScopeId,
    pub(super) arm: u8,
    pub(super) local_row: PackedLaneRange,
    pub(super) lane_step_row: PackedLaneRange,
    pub(super) arm_start: usize,
    pub(super) arm_end: usize,
}

impl RoleLaneScratch {
    #[inline(always)]
    pub(super) const fn same_scope(left: ScopeId, right: ScopeId) -> bool {
        !left.is_none() && left.canonical_raw() == right.canonical_raw()
    }

    #[inline(always)]
    pub(super) const fn first_enter_for_scope(markers: &[ScopeMarker], marker_idx: usize) -> bool {
        let marker = markers[marker_idx];
        if !matches!(marker.event, ScopeEvent::Enter) {
            return false;
        }
        let mut idx = 0usize;
        while idx < marker_idx {
            let candidate = markers[idx];
            if matches!(candidate.event, ScopeEvent::Enter)
                && Self::same_scope(candidate.scope_id, marker.scope_id)
            {
                return false;
            }
            idx += 1;
        }
        true
    }

    #[inline(always)]
    pub(super) const fn route_arm_ranges(
        markers: &[ScopeMarker],
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
            if Self::same_scope(marker.scope_id, route)
                && matches!(marker.scope_kind, ScopeKind::Route)
            {
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
                        if enter_len >= 2 && exit_len >= 2 {
                            break;
                        }
                    }
                }
            }
            idx += 1;
        }
        if enter_len == 2 && exit_len == 2 {
            Some([(starts[0], ends[0]), (starts[1], ends[1])])
        } else {
            None
        }
    }

    #[inline(always)]
    pub(super) const fn route_scope_slot_for_scope(
        markers: &[ScopeMarker],
        route: ScopeId,
    ) -> Option<usize> {
        if route.is_none() {
            return None;
        }
        let mut slot = 0usize;
        let mut idx = 0usize;
        while idx < markers.len() {
            let marker = markers[idx];
            if Self::first_enter_for_scope(markers, idx)
                && matches!(marker.scope_kind, ScopeKind::Route)
            {
                if Self::same_scope(marker.scope_id, route) {
                    return Some(slot);
                }
                slot += 1;
            }
            idx += 1;
        }
        None
    }

    #[inline(always)]
    pub(super) const fn scope_segment_end(
        markers: &[ScopeMarker],
        enter_idx: usize,
        segment_limit: usize,
    ) -> usize {
        let marker = markers[enter_idx];
        let mut scan = enter_idx + 1;
        while scan < markers.len() {
            let candidate = markers[scan];
            if Self::same_scope(candidate.scope_id, marker.scope_id)
                && matches!(candidate.event, ScopeEvent::Exit)
            {
                return candidate.offset;
            }
            scan += 1;
        }
        segment_limit
    }

    #[inline(always)]
    pub(super) const fn first_scope_segment_bounds(
        markers: &[ScopeMarker],
        segment_limit: usize,
        scope_id: ScopeId,
    ) -> Option<(ScopeKind, usize, usize)> {
        if scope_id.is_none() {
            return None;
        }
        let mut idx = 0usize;
        while idx < markers.len() {
            let marker = markers[idx];
            if matches!(marker.event, ScopeEvent::Enter)
                && Self::same_scope(marker.scope_id, scope_id)
            {
                return Some((
                    marker.scope_kind,
                    marker.offset,
                    Self::scope_segment_end(markers, idx, segment_limit),
                ));
            }
            idx += 1;
        }
        None
    }

    #[inline(always)]
    const fn scope_dependency_bounds(
        markers: &[ScopeMarker],
        segment_limit: usize,
        scope_id: ScopeId,
    ) -> Option<(ScopeKind, usize, usize)> {
        let Some((kind, start, end)) =
            Self::first_scope_segment_bounds(markers, segment_limit, scope_id)
        else {
            return None;
        };
        if !matches!(kind, ScopeKind::Route) {
            return Some((kind, start, end));
        }
        let Some(ranges) = Self::route_arm_ranges(markers, scope_id) else {
            crate::invariant();
        };
        let left = ranges[0];
        let right = ranges[1];
        let route_start = if left.0 < right.0 { left.0 } else { right.0 };
        let route_end = if left.1 > right.1 { left.1 } else { right.1 };
        Some((kind, route_start, route_end))
    }

    #[inline(always)]
    pub(super) const fn route_arm_for_scope_start(
        markers: &[ScopeMarker],
        route: ScopeId,
        start: usize,
    ) -> Option<u8> {
        let Some(ranges) = Self::route_arm_ranges(markers, route) else {
            return None;
        };
        let mut arm = 0usize;
        while arm < 2 {
            let (arm_start, arm_end) = ranges[arm];
            if arm_start <= start && start < arm_end {
                return Some(arm as u8);
            }
            arm += 1;
        }
        None
    }

    #[inline(always)]
    const fn route_arm_for_enter(markers: &[ScopeMarker], enter_idx: usize) -> Option<u8> {
        let marker = markers[enter_idx];
        if !matches!(marker.event, ScopeEvent::Enter)
            || !matches!(marker.scope_kind, ScopeKind::Route)
        {
            return None;
        }
        let mut arm = 0u8;
        let mut idx = 0usize;
        while idx < enter_idx {
            let candidate = markers[idx];
            if matches!(candidate.event, ScopeEvent::Enter)
                && matches!(candidate.scope_kind, ScopeKind::Route)
                && Self::same_scope(candidate.scope_id, marker.scope_id)
            {
                if arm == u8::MAX {
                    panic!("route arm ordinal overflow");
                }
                arm += 1;
            }
            idx += 1;
        }
        if arm <= 1 { Some(arm) } else { None }
    }

    #[inline(always)]
    pub(super) const fn nearest_route_for_scope(
        markers: &[ScopeMarker],
        segment_limit: usize,
        scope_id: ScopeId,
    ) -> Option<ScopeId> {
        let Some((_, target_start, target_end)) =
            Self::scope_dependency_bounds(markers, segment_limit, scope_id)
        else {
            return None;
        };
        let mut best = ScopeId::none();
        let mut best_span = usize::MAX;
        let mut best_start = 0usize;
        let mut idx = 0usize;
        while idx < markers.len() {
            let marker = markers[idx];
            if matches!(marker.event, ScopeEvent::Enter)
                && matches!(marker.scope_kind, ScopeKind::Route)
                && !Self::same_scope(marker.scope_id, scope_id)
            {
                let start = marker.offset;
                let end = match Self::route_arm_ranges(markers, marker.scope_id) {
                    Some(ranges) => {
                        let left_end = ranges[0].1;
                        let right_end = ranges[1].1;
                        if left_end > right_end {
                            left_end
                        } else {
                            right_end
                        }
                    }
                    None => crate::invariant(),
                };
                if start <= target_start && target_end <= end {
                    let span = end - start;
                    if best.is_none()
                        || span < best_span
                        || (span == best_span && start > best_start)
                    {
                        best = marker.scope_id;
                        best_span = span;
                        best_start = start;
                    }
                }
            }
            idx += 1;
        }
        if best.is_none() { None } else { Some(best) }
    }

    #[inline(always)]
    pub(super) const fn route_conflict_for_eff(
        markers: &[ScopeMarker],
        eff_idx: usize,
    ) -> PackedEventConflict {
        let mut best = ScopeId::none();
        let mut best_arm = 0u8;
        let mut best_span = usize::MAX;
        let mut best_start = 0usize;
        let mut idx = 0usize;
        while idx < markers.len() {
            let marker = markers[idx];
            if marker.offset > eff_idx {
                break;
            }
            if matches!(marker.event, ScopeEvent::Enter)
                && matches!(marker.scope_kind, ScopeKind::Route)
                && let Some(arm) = Self::route_arm_for_enter(markers, idx)
            {
                let start = marker.offset;
                let end = Self::scope_segment_end(markers, idx, usize::MAX);
                if start <= eff_idx && eff_idx < end {
                    let span = end - start;
                    if best.is_none()
                        || span < best_span
                        || (span == best_span && start > best_start)
                    {
                        best = marker.scope_id;
                        best_arm = arm;
                        best_span = span;
                        best_start = start;
                    }
                }
            }
            idx += 1;
        }
        if best.is_none() {
            PackedEventConflict::none()
        } else {
            PackedEventConflict::route_arm(best, best_arm)
        }
    }

    #[inline(always)]
    pub(super) const fn passive_arm_child_scope(
        markers: &[ScopeMarker],
        view_len: usize,
        route: ScopeId,
        arm: u8,
        arm_start: usize,
        arm_end: usize,
    ) -> Option<ScopeId> {
        if route.is_none() || arm > 1 || arm_start >= arm_end {
            return None;
        }
        let mut child = ScopeId::none();
        let mut child_span = usize::MAX;
        let mut idx = 0usize;
        while idx < markers.len() {
            let marker = markers[idx];
            if matches!(marker.event, ScopeEvent::Enter)
                && matches!(marker.scope_kind, ScopeKind::Route)
                && !Self::same_scope(marker.scope_id, route)
                && marker.offset == arm_start
            {
                let end = match Self::route_arm_ranges(markers, marker.scope_id) {
                    Some(ranges) => {
                        let left_end = ranges[0].1;
                        let right_end = ranges[1].1;
                        if left_end > right_end {
                            left_end
                        } else {
                            right_end
                        }
                    }
                    None => crate::invariant(),
                };
                if end <= arm_end {
                    let span = end - arm_start;
                    if child.is_none() || span > child_span {
                        child = marker.scope_id;
                        child_span = span;
                    }
                }
            }
            idx += 1;
        }
        if child.is_none() {
            None
        } else {
            let LocalConflict::RouteArm {
                scope: parent,
                arm: parent_arm,
            } = Self::dependency_conflict_for_scope(markers, view_len, child)
            else {
                panic!("passive child route has no parent conflict row");
            };
            if !Self::same_scope(parent, route) || parent_arm != arm {
                panic!("passive child row does not match conflict chain");
            }
            Some(child)
        }
    }

    #[inline(always)]
    pub(super) const fn push_route_arm_projection_row(
        &mut self,
        input: RouteArmProjectionRowInput<'_>,
    ) {
        let row_idx = input.route_slot * 2 + input.arm as usize;
        if row_idx >= MAX_ROUTE_ARM_LANE_ROWS {
            panic!("route arm projection row overflow");
        }
        let child_delta = match Self::passive_arm_child_scope(
            input.markers,
            input.view_len,
            input.route_scope,
            input.arm,
            input.arm_start,
            input.arm_end,
        ) {
            Some(child_scope) => {
                if Self::same_scope(child_scope, input.route_scope) {
                    panic!("passive route child scope must be a strict descendant");
                }
                let Some(child_slot) = Self::route_scope_slot_for_scope(input.markers, child_scope)
                else {
                    panic!("passive route child scope missing route row slot");
                };
                if child_slot <= input.route_slot {
                    panic!("passive route child scope must follow parent route slot");
                }
                Some(child_slot - input.route_slot)
            }
            None => None,
        };
        self.route_arm_rows[row_idx] =
            PackedRouteArmRow::new(input.local_row, child_delta, input.lane_step_row);
    }

    #[inline(always)]
    pub(super) const fn dependency_conflict_for_scope(
        markers: &[ScopeMarker],
        view_len: usize,
        scope: ScopeId,
    ) -> LocalConflict {
        match Self::nearest_route_for_scope(markers, view_len, scope) {
            Some(route) => {
                let Some((_, start, _)) = Self::scope_dependency_bounds(markers, view_len, scope)
                else {
                    return LocalConflict::SharedRoute;
                };
                LocalConflict::route_arm(
                    route,
                    Self::route_arm_for_scope_start(markers, route, start),
                )
            }
            None => LocalConflict::Unconditional,
        }
    }
}
