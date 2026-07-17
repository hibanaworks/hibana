use super::{
    super::{
        PackedLaneRange, PackedLocalEventRow, ScopeEvent, ScopeId, ScopeKind, ScopeMarkerView,
    },
    binary_route_arm_index,
};
use crate::global::{
    const_dsl::{
        EffList, StructuredScopeRange, innermost_scope_range, parallel_arm_ranges_from_enter,
        structured_scope_event_range,
    },
    typestate::{LocalConflict, PackedEventConflict, RouteChoiceMark},
};

mod dependency;
mod lanes;
mod resident;
mod route;
pub(super) use crate::global::const_dsl::route_arm_event_ranges_for_scope as route_arm_ranges;
pub(super) use crate::global::const_dsl::scope_segment_end_from_enter as scope_segment_end;
pub(super) use crate::global::const_dsl::{
    passive_route_child_scope as passive_arm_child_scope, route_scope_slot_for_scope,
};
pub(super) use dependency::DependencyCursor;
pub(super) use lanes::{LANE_BITMAP_BYTES, LocalLaneFacts};
pub(super) use resident::ResidentRowCursor;
pub(super) use route::{route_commit_conflict_at, route_commit_row_count};

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

const fn scope_dependency_bounds(
    markers: ScopeMarkerView<'_>,
    segment_limit: usize,
    scope_id: ScopeId,
) -> Option<(ScopeKind, usize, usize)> {
    let kind = match scope_id.kind() {
        Some(kind) => kind,
        None => return None,
    };
    let (start, end) = match structured_scope_event_range(markers, scope_id) {
        Some(range) => range,
        None => crate::invariant(),
    };
    if end > segment_limit {
        crate::invariant();
    }
    Some((kind, start, end))
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
    if !matches!(marker.scope_id.kind(), Some(ScopeKind::Route)) {
        return None;
    }
    marker.event.route_arm()
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
    let mut best: Option<StructuredScopeRange> = None;
    let mut idx = 0usize;
    while idx < markers.len() {
        let marker = markers.at(idx);
        if markers.is_first_enter(idx)
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
                let candidate = StructuredScopeRange::new(marker.scope_id, start, end);
                best = Some(match best {
                    Some(selected) => innermost_scope_range(selected, candidate),
                    None => candidate,
                });
            }
        }
        idx += 1;
    }
    match best {
        Some(selected) => Some(selected.scope()),
        None => None,
    }
}

pub(super) const fn route_conflict_for_eff(
    markers: ScopeMarkerView<'_>,
    eff_idx: usize,
) -> PackedEventConflict {
    let mut best: Option<StructuredScopeRange> = None;
    let mut best_arm = 0u8;
    let mut idx = 0usize;
    while idx < markers.len() {
        let marker = markers.at(idx);
        if marker.offset() > eff_idx {
            break;
        }
        if marker.event.is_enter()
            && matches!(marker.scope_id.kind(), Some(ScopeKind::Route))
            && let Some(arm) = route_arm_for_enter(markers, idx)
        {
            let start = marker.offset();
            let end = scope_segment_end(markers, idx, None);
            if start <= eff_idx && eff_idx < end {
                let candidate = StructuredScopeRange::new(marker.scope_id, start, end);
                let selected = match best {
                    Some(current) => innermost_scope_range(current, candidate),
                    None => candidate,
                };
                if selected.scope().same(candidate.scope()) {
                    best_arm = arm;
                }
                best = Some(selected);
            }
        }
        idx += 1;
    }
    match best {
        Some(selected) => PackedEventConflict::route_arm(selected.scope(), best_arm),
        None => PackedEventConflict::none(),
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
    let Some((_, _, _, end)) = parallel_arm_ranges_from_enter(markers, enter_idx) else {
        panic!("parallel scope enter expected");
    };
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
            ScopeEvent::Enter(_) => {
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
    let mut local_start = None;
    let mut local_len = 0usize;
    let limit = if end_eff < eff_list.len() {
        end_eff
    } else {
        eff_list.len()
    };
    let mut eff_idx = 0usize;
    while eff_idx < limit {
        let atom = eff_list.atom_at(eff_idx);
        if atom.from == role || atom.to == role {
            if eff_idx >= start_eff && eff_idx < end_eff {
                if local_start.is_none() {
                    local_start = Some(local_step);
                }
                local_len += 1;
            }
            local_step += 1;
        }
        eff_idx += 1;
    }
    match local_start {
        Some(start) => PackedLaneRange::new(start, local_len),
        None => PackedLaneRange::new(0, 0),
    }
}

pub(super) const fn scope_at<const E: usize>(eff_list: &EffList<E>, eff_idx: usize) -> ScopeId {
    let markers = eff_list.scope_markers();
    let mut best: Option<StructuredScopeRange> = None;
    let mut idx = 0usize;
    while idx < markers.len() {
        let marker = markers.at(idx);
        if marker.offset() > eff_idx {
            break;
        }
        if marker.event.is_enter() {
            let start = marker.offset();
            let end = scope_segment_end(markers, idx, Some(eff_list.len()));
            if eff_idx < end {
                let candidate = StructuredScopeRange::new(marker.scope_id, start, end);
                best = Some(match best {
                    Some(selected) => innermost_scope_range(selected, candidate),
                    None => candidate,
                });
            }
        }
        idx += 1;
    }
    match best {
        Some(selected) => selected.scope(),
        None => ScopeId::none(),
    }
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
        let atom = eff_list.atom_at(idx);
        if atom.to == role && atom.from != role {
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
