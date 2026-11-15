use crate::{
    g::ProgramSourceError,
    global::const_dsl::{
        EffList, ScopeKind, ScopeMarkerView, closed_route_arm_ranges_from_first_enter,
        passive_route_child_enter,
    },
};

pub(crate) const fn validate_passive_child_projection_guarantees<const E: usize>(
    source: &EffList<E>,
) -> Option<ProgramSourceError> {
    let markers = source.scope_markers();
    if markers.len() > u16::MAX as usize {
        return Some(ProgramSourceError::ProjectionRouteUnprojectable);
    }
    // These are checker facts derived from source intervals, independently of
    // the projection writer's parent table. Each marker is indexed once.
    let mut slots = [u16::MAX; E];
    let mut parents = [u16::MAX; E];
    let mut slot = 0;
    let mut index = 0;
    while index < markers.len() {
        let marker = markers.at(index);
        if marker.event.is_primary_enter()
            && matches!(marker.scope_id.kind(), Some(ScopeKind::Route))
        {
            if closed_route_arm_ranges_from_first_enter(markers, index).is_none()
                || slot >= u16::MAX as usize
            {
                return Some(ProgramSourceError::ProjectionRouteUnprojectable);
            }
            slots[index] = slot as u16;
            slot += 1;
        }
        index += 1;
    }
    let mut active = [0u16; E];
    let mut depth = 0;
    index = 0;
    while index < markers.len() {
        let start = markers.at(index).offset();
        while depth != 0 && markers.at(active[depth - 1] as usize).segment_end() <= start {
            depth -= 1;
        }
        let mut limit = index + 1;
        while limit < markers.len() && markers.at(limit).offset() == start {
            limit += 1;
        }
        // Collect all route arms at this boundary. Descending ends put the
        // tightest covering interval last; source ordinals resolve equal ends.
        let mut current = index;
        while current < limit {
            let marker = markers.at(current);
            if matches!(marker.scope_id.kind(), Some(ScopeKind::Route))
                && marker.event.route_arm().is_some()
            {
                let mut insert = depth;
                while insert != 0 {
                    let previous = markers.at(active[insert - 1] as usize);
                    if previous.segment_end() > marker.segment_end()
                        || (previous.segment_end() == marker.segment_end()
                            && (previous.offset() < marker.offset()
                                || (previous.offset() == marker.offset()
                                    && previous.scope_id.local_ordinal()
                                        < marker.scope_id.local_ordinal())))
                    {
                        break;
                    }
                    active[insert] = active[insert - 1];
                    insert -= 1;
                }
                active[insert] = current as u16;
                depth += 1;
            }
            current += 1;
        }
        current = index;
        while current < limit {
            if slots[current] != u16::MAX {
                let child = markers.at(current);
                let Some([_, (_, end)]) =
                    closed_route_arm_ranges_from_first_enter(markers, current)
                else {
                    return Some(ProgramSourceError::ProjectionRouteUnprojectable);
                };
                let mut low = 0;
                let mut high = depth;
                while low < high {
                    let middle = low + (high - low) / 2;
                    if markers.at(active[middle] as usize).segment_end() >= end {
                        low = middle + 1;
                    } else {
                        high = middle;
                    }
                }
                while low != 0 {
                    low -= 1;
                    let parent = active[low] as usize;
                    if !markers.at(parent).scope_id.same(child.scope_id) {
                        parents[current] = parent as u16;
                        break;
                    }
                }
            }
            current += 1;
        }
        index = limit;
    }
    index = 0;
    while index < markers.len() {
        if slots[index] != u16::MAX {
            let mut arm = 0;
            while arm < 2 {
                if let Some(error) = validate_child(markers, (index, arm), (&parents, &slots)) {
                    return Some(error);
                }
                arm += 1;
            }
        }
        index += 1;
    }
    None
}

const fn validate_child(
    markers: ScopeMarkerView<'_>,
    (route_enter, arm): (usize, u8),
    (parents, slots): (&[u16], &[u16]),
) -> Option<ProgramSourceError> {
    let Some(child_enter) = passive_route_child_enter(markers, route_enter, arm) else {
        return None;
    };
    let route = markers.at(route_enter).scope_id;
    let child = markers.at(child_enter).scope_id;
    let parent = parents[child_enter];
    if child.same(route) || parent == u16::MAX {
        return Some(ProgramSourceError::ProjectionRouteUnprojectable);
    }
    let parent = markers.at(parent as usize);
    if !parent.scope_id.same(route)
        || !matches!(parent.event.route_arm(), Some(found) if found == arm)
        || slots[child_enter] <= slots[route_enter]
        || slots[child_enter] == u16::MAX
    {
        return Some(ProgramSourceError::ProjectionRouteUnprojectable);
    }
    None
}
