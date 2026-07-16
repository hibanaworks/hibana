use super::{
    super::{
        PackedLaneRange, PackedLocalEventRow, ScopeEvent, ScopeId, ScopeKind, ScopeMarkerView,
    },
    binary_route_arm_index,
};
use crate::global::{
    const_dsl::EffList,
    typestate::{LocalConflict, PackedEventConflict, RouteChoiceMark},
};

mod dependency;
mod lanes;
mod resident;
mod route;
pub(super) use dependency::DependencyCursor;
pub(super) use lanes::{LANE_BITMAP_BYTES, LocalLaneFacts};
pub(super) use resident::ResidentRowCursor;
pub(super) use route::{
    passive_arm_child_scope, route_commit_conflict_at, route_commit_row_count,
    route_scope_slot_for_scope,
};

pub(super) const fn same_scope(left: ScopeId, right: ScopeId) -> bool {
    !left.is_none() && left.same(right)
}

pub(super) const fn scope_markers_contain_kind(
    markers: ScopeMarkerView<'_>,
    kind: ScopeKind,
) -> bool {
    let mut marker_idx = 0usize;
    while marker_idx < markers.len() {
        let marker = markers.at(marker_idx);
        if let Some(candidate) = marker.scope_id.kind()
            && candidate as u16 == kind as u16
        {
            return true;
        }
        marker_idx += 1;
    }
    false
}

pub(super) const fn first_enter_for_scope(markers: ScopeMarkerView<'_>, marker_idx: usize) -> bool {
    let marker = markers.at(marker_idx);
    if !matches!(marker.event, ScopeEvent::Enter) {
        return false;
    }
    let mut idx = 0usize;
    while idx < marker_idx {
        let candidate = markers.at(idx);
        if matches!(candidate.event, ScopeEvent::Enter)
            && same_scope(candidate.scope_id, marker.scope_id)
        {
            return false;
        }
        idx += 1;
    }
    true
}

pub(super) const fn route_arm_ranges(
    markers: ScopeMarkerView<'_>,
    route: ScopeId,
) -> Option<[(usize, usize); 2]> {
    if route.is_none() {
        return None;
    }
    let mut starts = [usize::MAX; 2];
    let mut ends = [usize::MAX; 2];
    let mut enter_len = 0usize;
    let mut idx = 0usize;
    while idx < markers.len() {
        let marker = markers.at(idx);
        if same_scope(marker.scope_id, route)
            && matches!(marker.scope_id.kind(), Some(ScopeKind::Route))
        {
            match marker.event {
                ScopeEvent::Enter => {
                    if enter_len < 2 {
                        starts[enter_len] = marker.offset();
                        ends[enter_len] = marker.segment_end();
                    }
                    enter_len += 1;
                }
                ScopeEvent::Exit => {
                    if enter_len >= 2 {
                        break;
                    }
                }
                ScopeEvent::Split => {}
            }
        }
        idx += 1;
    }
    if enter_len == 2 {
        Some([(starts[0], ends[0]), (starts[1], ends[1])])
    } else {
        None
    }
}

pub(super) const fn scope_segment_end(
    markers: ScopeMarkerView<'_>,
    enter_idx: usize,
    segment_limit: usize,
) -> usize {
    let marker = markers.at(enter_idx);
    let end = marker.segment_end();
    if end <= marker.offset() || end > segment_limit {
        crate::invariant();
    }
    end
}

pub(super) const fn first_scope_segment_bounds(
    markers: ScopeMarkerView<'_>,
    segment_limit: usize,
    scope_id: ScopeId,
) -> Option<(ScopeKind, usize, usize)> {
    if scope_id.is_none() {
        return None;
    }
    let mut idx = 0usize;
    while idx < markers.len() {
        let marker = markers.at(idx);
        if matches!(marker.event, ScopeEvent::Enter) && same_scope(marker.scope_id, scope_id) {
            return Some((
                match marker.scope_id.kind() {
                    Some(kind) => kind,
                    None => return None,
                },
                marker.offset(),
                scope_segment_end(markers, idx, segment_limit),
            ));
        }
        idx += 1;
    }
    None
}

const fn scope_dependency_bounds(
    markers: ScopeMarkerView<'_>,
    segment_limit: usize,
    scope_id: ScopeId,
) -> Option<(ScopeKind, usize, usize)> {
    let Some((kind, start, end)) = first_scope_segment_bounds(markers, segment_limit, scope_id)
    else {
        return None;
    };
    if !matches!(kind, ScopeKind::Route) {
        return Some((kind, start, end));
    }
    let Some(ranges) = route_arm_ranges(markers, scope_id) else {
        crate::invariant();
    };
    let left = ranges[0];
    let right = ranges[1];
    Some((
        kind,
        if left.0 < right.0 { left.0 } else { right.0 },
        if left.1 > right.1 { left.1 } else { right.1 },
    ))
}

pub(super) const fn route_arm_for_scope_start(
    markers: ScopeMarkerView<'_>,
    route: ScopeId,
    start: usize,
) -> Option<u8> {
    let Some(ranges) = route_arm_ranges(markers, route) else {
        crate::invariant();
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

const fn route_arm_for_enter(markers: ScopeMarkerView<'_>, enter_idx: usize) -> Option<u8> {
    let marker = markers.at(enter_idx);
    if !matches!(marker.event, ScopeEvent::Enter)
        || !matches!(marker.scope_id.kind(), Some(ScopeKind::Route))
    {
        return None;
    }
    let mut arm = 0u8;
    let mut idx = 0usize;
    while idx < enter_idx {
        let candidate = markers.at(idx);
        if matches!(candidate.event, ScopeEvent::Enter)
            && matches!(candidate.scope_id.kind(), Some(ScopeKind::Route))
            && same_scope(candidate.scope_id, marker.scope_id)
        {
            if arm == u8::MAX {
                panic!("route arm ordinal overflow");
            }
            arm += 1;
        }
        idx += 1;
    }
    Some(binary_route_arm_index(arm) as u8)
}

pub(super) const fn nearest_route_for_scope(
    markers: ScopeMarkerView<'_>,
    segment_limit: usize,
    scope_id: ScopeId,
) -> Option<ScopeId> {
    let Some((_, target_start, target_end)) =
        scope_dependency_bounds(markers, segment_limit, scope_id)
    else {
        return None;
    };
    let mut best = ScopeId::none();
    let mut best_span = usize::MAX;
    let mut best_start = 0usize;
    let mut idx = 0usize;
    while idx < markers.len() {
        let marker = markers.at(idx);
        if matches!(marker.event, ScopeEvent::Enter)
            && matches!(marker.scope_id.kind(), Some(ScopeKind::Route))
            && !same_scope(marker.scope_id, scope_id)
        {
            let start = marker.offset();
            let end = match route_arm_ranges(markers, marker.scope_id) {
                Some(ranges) => {
                    if ranges[0].1 > ranges[1].1 {
                        ranges[0].1
                    } else {
                        ranges[1].1
                    }
                }
                None => crate::invariant(),
            };
            if start <= target_start && target_end <= end {
                let span = end - start;
                if best.is_none() || span < best_span || (span == best_span && start > best_start) {
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

pub(super) const fn route_conflict_for_eff(
    markers: ScopeMarkerView<'_>,
    eff_idx: usize,
) -> PackedEventConflict {
    let mut best = ScopeId::none();
    let mut best_arm = 0u8;
    let mut best_span = usize::MAX;
    let mut best_start = 0usize;
    let mut idx = 0usize;
    while idx < markers.len() {
        let marker = markers.at(idx);
        if marker.offset() > eff_idx {
            break;
        }
        if matches!(marker.event, ScopeEvent::Enter)
            && matches!(marker.scope_id.kind(), Some(ScopeKind::Route))
            && let Some(arm) = route_arm_for_enter(markers, idx)
        {
            let start = marker.offset();
            let end = scope_segment_end(markers, idx, usize::MAX);
            if start <= eff_idx && eff_idx < end {
                let span = end - start;
                if best.is_none() || span < best_span || (span == best_span && start > best_start) {
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

pub(super) const fn dependency_conflict_for_scope(
    markers: ScopeMarkerView<'_>,
    view_len: usize,
    scope: ScopeId,
) -> LocalConflict {
    match nearest_route_for_scope(markers, view_len, scope) {
        Some(route) => {
            let Some((_, start, _)) = scope_dependency_bounds(markers, view_len, scope) else {
                crate::invariant();
            };
            LocalConflict::route_arm(route, route_arm_for_scope_start(markers, route, start))
        }
        None => LocalConflict::Unconditional,
    }
}

pub(super) const fn parallel_exit_for_enter(
    markers: ScopeMarkerView<'_>,
    enter_idx: usize,
) -> usize {
    let marker = markers.at(enter_idx);
    if !matches!(marker.event, ScopeEvent::Enter)
        || !matches!(marker.scope_id.kind(), Some(ScopeKind::Parallel))
    {
        panic!("parallel scope enter expected");
    }
    let end = marker.segment_end();
    if end <= marker.offset() {
        panic!("parallel scope segment is empty");
    }
    end
}

pub(super) const fn nearest_parent_parallel_end(
    markers: ScopeMarkerView<'_>,
    enter_idx: usize,
    exit_eff: usize,
) -> usize {
    let mut depth = 0usize;
    let mut scan = enter_idx;
    while scan > 0 {
        scan -= 1;
        let candidate = markers.at(scan);
        match candidate.event {
            ScopeEvent::Exit => depth += 1,
            ScopeEvent::Enter => {
                if depth == 0 {
                    if matches!(candidate.scope_id.kind(), Some(ScopeKind::Parallel)) {
                        return parallel_exit_for_enter(markers, scan);
                    }
                } else {
                    depth -= 1;
                }
            }
            ScopeEvent::Split => {}
        }
    }
    exit_eff
}

pub(super) const fn local_step_range_for_eff_range<const E: usize>(
    eff_list: &EffList<E>,
    start_eff: usize,
    end_eff: usize,
    role: u8,
) -> PackedLaneRange {
    if start_eff >= end_eff {
        return PackedLaneRange::new(0, 0);
    }
    let mut local_step = 0usize;
    let mut local_start = usize::MAX;
    let mut local_len = 0usize;
    let limit = if end_eff < eff_list.len() {
        end_eff
    } else {
        eff_list.len()
    };
    let mut eff_idx = 0usize;
    while eff_idx < limit {
        let node = eff_list.node_at(eff_idx);
        if matches!(node.kind, crate::eff::EffKind::Atom) {
            let atom = node.atom_data();
            if atom.from == role || atom.to == role {
                if eff_idx >= start_eff && eff_idx < end_eff {
                    if local_start == usize::MAX {
                        local_start = local_step;
                    }
                    local_len += 1;
                }
                local_step += 1;
            }
        }
        eff_idx += 1;
    }
    if local_start == usize::MAX {
        PackedLaneRange::new(0, 0)
    } else {
        PackedLaneRange::new(local_start, local_len)
    }
}

pub(super) const fn scope_at<const E: usize>(eff_list: &EffList<E>, eff_idx: usize) -> ScopeId {
    let markers = eff_list.scope_markers();
    let mut best = ScopeId::none();
    let mut best_start = 0usize;
    let mut best_span = usize::MAX;
    let mut idx = 0usize;
    while idx < markers.len() {
        let marker = markers.at(idx);
        if marker.offset() > eff_idx {
            break;
        }
        if matches!(marker.event, ScopeEvent::Enter) {
            let start = marker.offset();
            let end = scope_segment_end(markers, idx, eff_list.len());
            if eff_idx < end {
                if best.is_none() || start > best_start {
                    best = marker.scope_id;
                    best_start = start;
                    best_span = usize::MAX;
                } else if start == best_start {
                    let span = end - start;
                    if span < best_span {
                        best = marker.scope_id;
                        best_start = start;
                        best_span = span;
                    }
                }
            }
        }
        idx += 1;
    }
    best
}

const fn route_scope_and_arm_at<const E: usize>(
    eff_list: &EffList<E>,
    eff_idx: usize,
) -> Option<(ScopeId, u8)> {
    match route_conflict_for_eff(eff_list.scope_markers(), eff_idx).to_conflict() {
        Some(LocalConflict::RouteArm { scope, arm }) => Some((scope, arm)),
        Some(LocalConflict::Unconditional | LocalConflict::SharedRoute) | None => None,
    }
}

const fn first_recv_eff_for_route_arm<const E: usize>(
    eff_list: &EffList<E>,
    route: ScopeId,
    arm: u8,
    role: u8,
) -> Option<usize> {
    let arm = binary_route_arm_index(arm);
    let Some(ranges) = route_arm_ranges(eff_list.scope_markers(), route) else {
        crate::invariant();
    };
    let (start, end) = ranges[arm];
    let mut idx = start;
    while idx < end && idx < eff_list.len() {
        let node = eff_list.node_at(idx);
        if matches!(node.kind, crate::eff::EffKind::Atom) && {
            let atom = node.atom_data();
            atom.to == role && atom.from != role
        } {
            return Some(idx);
        }
        idx += 1;
    }
    None
}

pub(super) const fn local_event_row_for_eff<const E: usize>(
    eff_list: &EffList<E>,
    eff_idx: usize,
    frame_label: u8,
    role: u8,
) -> PackedLocalEventRow {
    let scope = scope_at(eff_list, eff_idx);
    let choice = match route_scope_and_arm_at(eff_list, eff_idx) {
        Some((route_scope, arm)) => {
            match first_recv_eff_for_route_arm(eff_list, route_scope, arm, role) {
                Some(first) if first == eff_idx => RouteChoiceMark::Determinant,
                Some(_) | None => RouteChoiceMark::Ordinary,
            }
        }
        None => RouteChoiceMark::Ordinary,
    };
    PackedLocalEventRow::new(eff_idx, scope, frame_label, choice)
}
