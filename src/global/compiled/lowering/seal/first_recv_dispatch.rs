use crate::{
    eff,
    g::ProgramSourceError,
    global::{
        compiled::lowering::CompiledProgramView,
        const_dsl::{EffList, ScopeEvent, ScopeId, ScopeKind, ScopeMarker},
        typestate::{MAX_FIRST_RECV_DISPATCH, PackedEventConflict},
    },
};

#[derive(Clone, Copy)]
struct FirstRecvDispatchVisit {
    enter_idx: usize,
    scope: ScopeId,
    arm: u8,
    root_arm: u8,
}

impl FirstRecvDispatchVisit {
    const EMPTY: Self = Self {
        enter_idx: usize::MAX,
        scope: ScopeId::none(),
        arm: 0,
        root_arm: 0,
    };
}

#[derive(Clone, Copy)]
struct FirstRecvDispatchSpecSeal {
    lane: u8,
    label: u8,
    root_arm: u8,
    eff_idx: usize,
}

impl FirstRecvDispatchSpecSeal {
    const EMPTY: Self = Self {
        lane: 0,
        label: 0,
        root_arm: 0,
        eff_idx: usize::MAX,
    };
}

#[inline(always)]
pub(super) const fn validate_first_recv_dispatch_capacity<const ROLE: u8>(
    view: &CompiledProgramView<'_>,
    eff_list: &EffList,
) -> Option<ProgramSourceError> {
    let scope_markers = view.scope_markers();
    let mut seen_routes = [false; eff::meta::MAX_EFF_NODES];
    let mut marker_idx = 0usize;
    while marker_idx < scope_markers.len() {
        let marker = scope_markers[marker_idx];
        if matches!(marker.event, ScopeEvent::Enter)
            && matches!(marker.scope_kind, ScopeKind::Route)
        {
            let ordinal = marker.scope_id.local_ordinal() as usize;
            if ordinal >= seen_routes.len() {
                return Some(ProgramSourceError::ProjectionRouteUnprojectable);
            }
            if !seen_routes[ordinal] {
                seen_routes[ordinal] = true;
                if let Some(error) = validate_first_recv_dispatch_scope::<ROLE>(
                    scope_markers,
                    eff_list,
                    marker_idx,
                    marker.scope_id,
                ) {
                    return Some(error);
                }
            }
        }
        marker_idx += 1;
    }
    None
}

const fn validate_first_recv_dispatch_scope<const ROLE: u8>(
    scope_markers: &[ScopeMarker],
    eff_list: &EffList,
    scope_enter_idx: usize,
    scope: ScopeId,
) -> Option<ProgramSourceError> {
    let mut table = [FirstRecvDispatchSpecSeal::EMPTY; MAX_FIRST_RECV_DISPATCH];
    let mut table_len = 0usize;
    let mut visits = [FirstRecvDispatchVisit::EMPTY; MAX_FIRST_RECV_DISPATCH];
    let mut visit_len = 0usize;

    if !push_first_recv_dispatch_visit(
        &mut visits,
        &mut visit_len,
        FirstRecvDispatchVisit {
            enter_idx: scope_enter_idx,
            scope,
            arm: 1,
            root_arm: 1,
        },
    ) {
        return Some(ProgramSourceError::ProjectionRouteUnprojectable);
    }
    if !push_first_recv_dispatch_visit(
        &mut visits,
        &mut visit_len,
        FirstRecvDispatchVisit {
            enter_idx: scope_enter_idx,
            scope,
            arm: 0,
            root_arm: 0,
        },
    ) {
        return Some(ProgramSourceError::ProjectionRouteUnprojectable);
    }

    let mut depth = 0usize;
    while visit_len > 0 {
        if depth >= PackedEventConflict::MAX_CHAIN_DEPTH {
            return Some(ProgramSourceError::ProjectionRouteUnprojectable);
        }
        visit_len -= 1;
        let visit = visits[visit_len];
        depth += 1;

        let (_, arm0_start, arm0_end, _, arm1_start, arm1_end) =
            route_arm_ranges_from_first_enter(scope_markers, visit.enter_idx);
        let (arm_start, arm_end) = if visit.arm == 0 {
            (arm0_start, arm0_end)
        } else {
            (arm1_start, arm1_end)
        };
        if let Some(spec) =
            first_recv_dispatch_spec_for_arm::<ROLE>(eff_list, visit.root_arm, arm_start, arm_end)
        {
            if !push_first_recv_dispatch_spec(&mut table, &mut table_len, spec) {
                return Some(ProgramSourceError::ProjectionRouteUnprojectable);
            }
        }
        if let Some(child_enter_idx) = passive_child_route_enter_index(
            scope_markers,
            visit.scope,
            visit.arm,
            arm_start,
            arm_end,
        ) {
            let child_scope = scope_markers[child_enter_idx].scope_id;
            if !push_first_recv_dispatch_visit(
                &mut visits,
                &mut visit_len,
                FirstRecvDispatchVisit {
                    enter_idx: child_enter_idx,
                    scope: child_scope,
                    arm: 1,
                    root_arm: visit.root_arm,
                },
            ) {
                return Some(ProgramSourceError::ProjectionRouteUnprojectable);
            }
            if !push_first_recv_dispatch_visit(
                &mut visits,
                &mut visit_len,
                FirstRecvDispatchVisit {
                    enter_idx: child_enter_idx,
                    scope: child_scope,
                    arm: 0,
                    root_arm: visit.root_arm,
                },
            ) {
                return Some(ProgramSourceError::ProjectionRouteUnprojectable);
            }
        }
    }
    None
}

const fn push_first_recv_dispatch_visit(
    visits: &mut [FirstRecvDispatchVisit; MAX_FIRST_RECV_DISPATCH],
    len: &mut usize,
    visit: FirstRecvDispatchVisit,
) -> bool {
    if *len >= visits.len() {
        return false;
    }
    visits[*len] = visit;
    *len += 1;
    true
}

const fn push_first_recv_dispatch_spec(
    table: &mut [FirstRecvDispatchSpecSeal; MAX_FIRST_RECV_DISPATCH],
    len: &mut usize,
    spec: FirstRecvDispatchSpecSeal,
) -> bool {
    let mut idx = 0usize;
    while idx < *len {
        let entry = table[idx];
        if entry.lane == spec.lane
            && entry.label == spec.label
            && entry.root_arm == spec.root_arm
            && entry.eff_idx == spec.eff_idx
        {
            return true;
        }
        idx += 1;
    }
    if *len >= table.len() {
        return false;
    }
    table[*len] = spec;
    *len += 1;
    true
}

const fn first_recv_dispatch_spec_for_arm<const ROLE: u8>(
    eff_list: &EffList,
    root_arm: u8,
    start: usize,
    end: usize,
) -> Option<FirstRecvDispatchSpecSeal> {
    let mut idx = start;
    while idx < end && idx < eff_list.len() {
        let node = eff_list.node_at(idx);
        if matches!(node.kind, eff::EffKind::Atom) {
            let atom = node.atom_data();
            if atom.to == ROLE && atom.from != ROLE {
                return Some(FirstRecvDispatchSpecSeal {
                    lane: atom.lane,
                    label: atom.label,
                    root_arm,
                    eff_idx: idx,
                });
            }
        }
        idx += 1;
    }
    None
}

const fn passive_child_route_enter_index(
    scope_markers: &[ScopeMarker],
    route: ScopeId,
    arm: u8,
    arm_start: usize,
    arm_end: usize,
) -> Option<usize> {
    if route.is_none() || arm > 1 || arm_start >= arm_end {
        return None;
    }
    let mut child_idx = usize::MAX;
    let mut child_span = usize::MAX;
    let mut idx = 0usize;
    while idx < scope_markers.len() {
        let marker = scope_markers[idx];
        if matches!(marker.event, ScopeEvent::Enter)
            && matches!(marker.scope_kind, ScopeKind::Route)
            && marker.scope_id.canonical().raw() != route.canonical().raw()
            && marker.offset == arm_start
            && first_enter_for_scope(scope_markers, idx)
        {
            let (_, _, left_end, _, _, right_end) =
                route_arm_ranges_from_first_enter(scope_markers, idx);
            let end = if left_end > right_end {
                left_end
            } else {
                right_end
            };
            if end <= arm_end {
                let span = end - arm_start;
                if child_idx == usize::MAX || span > child_span {
                    child_idx = idx;
                    child_span = span;
                }
            }
        }
        idx += 1;
    }
    if child_idx == usize::MAX {
        None
    } else {
        Some(child_idx)
    }
}

const fn first_enter_for_scope(scope_markers: &[ScopeMarker], marker_idx: usize) -> bool {
    let marker = scope_markers[marker_idx];
    if !matches!(marker.event, ScopeEvent::Enter) {
        return false;
    }
    let mut idx = 0usize;
    while idx < marker_idx {
        let candidate = scope_markers[idx];
        if matches!(candidate.event, ScopeEvent::Enter)
            && candidate.scope_id.canonical().raw() == marker.scope_id.canonical().raw()
        {
            return false;
        }
        idx += 1;
    }
    true
}

#[inline(always)]
const fn route_arm_ranges_from_first_enter(
    scope_markers: &[ScopeMarker],
    enter_idx: usize,
) -> (usize, usize, usize, usize, usize, usize) {
    if enter_idx >= scope_markers.len() {
        panic!("route enter marker index out of bounds");
    }
    let route = scope_markers[enter_idx].scope_id;
    let mut enter_marker_indices = [usize::MAX; 2];
    let mut enter_offsets = [usize::MAX; 2];
    let mut exit_offsets = [usize::MAX; 2];
    let mut enter_len = 1usize;
    let mut exit_len = 0usize;
    enter_marker_indices[0] = enter_idx;
    enter_offsets[0] = scope_markers[enter_idx].offset;

    let mut idx = enter_idx + 1;
    while idx < scope_markers.len() && (enter_len < 2 || exit_len < 2) {
        let marker = scope_markers[idx];
        if marker.scope_id.canonical().raw() == route.canonical().raw()
            && matches!(marker.scope_kind, ScopeKind::Route)
        {
            match marker.event {
                ScopeEvent::Enter => {
                    if enter_len < 2 {
                        enter_marker_indices[enter_len] = idx;
                        enter_offsets[enter_len] = marker.offset;
                    }
                    enter_len += 1;
                }
                ScopeEvent::Exit => {
                    if exit_len < 2 {
                        exit_offsets[exit_len] = marker.offset;
                    }
                    exit_len += 1;
                }
            }
        }
        idx += 1;
    }

    if enter_len != 2 || exit_len != 2 {
        panic!("route must have exactly 2 arms");
    }
    (
        enter_marker_indices[0],
        enter_offsets[0],
        exit_offsets[0],
        enter_marker_indices[1],
        enter_offsets[1],
        exit_offsets[1],
    )
}
