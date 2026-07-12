use super::{
    EffList, ScopeEvent, ScopeKind, ScopeMarker, parallel_arm_ranges_from_enter,
    route_arm_ranges_from_first_enter,
};

const ABSENT_WITNESS: u16 = u16::MAX;

const fn is_first_route_enter(markers: &[ScopeMarker], marker_idx: usize) -> bool {
    let marker = markers[marker_idx];
    if !matches!(marker.event, ScopeEvent::Enter)
        || !matches!(marker.scope_id.kind(), Some(ScopeKind::Route))
    {
        return false;
    }
    let mut idx = 0usize;
    while idx < marker_idx {
        let candidate = markers[idx];
        if matches!(candidate.event, ScopeEvent::Enter) && candidate.scope_id.same(marker.scope_id)
        {
            return false;
        }
        idx += 1;
    }
    true
}

const fn route_arm_at(
    markers: &[ScopeMarker],
    route_enter_idx: usize,
    eff_idx: usize,
) -> Option<u8> {
    let (_, left_start, left_end, _, right_start, right_end) =
        route_arm_ranges_from_first_enter(markers, route_enter_idx);
    if left_start <= eff_idx && eff_idx < left_end {
        Some(0)
    } else if right_start <= eff_idx && eff_idx < right_end {
        Some(1)
    } else {
        None
    }
}

const fn parallel_arm_at(
    markers: &[ScopeMarker],
    parallel_enter_idx: usize,
    eff_idx: usize,
) -> Option<u8> {
    let Some((left_start, left_end, right_start, right_end)) =
        parallel_arm_ranges_from_enter(markers, parallel_enter_idx)
    else {
        panic!("parallel scope arm range missing");
    };
    if left_start <= eff_idx && eff_idx < left_end {
        Some(0)
    } else if right_start <= eff_idx && eff_idx < right_end {
        Some(1)
    } else {
        None
    }
}

const fn mutually_exclusive_route_arms(
    markers: &[ScopeMarker],
    left_eff_idx: usize,
    right_eff_idx: usize,
) -> bool {
    let mut marker_idx = 0usize;
    while marker_idx < markers.len() {
        if is_first_route_enter(markers, marker_idx) {
            let left_arm = route_arm_at(markers, marker_idx, left_eff_idx);
            let right_arm = route_arm_at(markers, marker_idx, right_eff_idx);
            if matches!(
                (left_arm, right_arm),
                (Some(0), Some(1)) | (Some(1), Some(0))
            ) {
                return true;
            }
        }
        marker_idx += 1;
    }
    false
}

const fn different_parallel_arms(
    markers: &[ScopeMarker],
    left_eff_idx: usize,
    right_eff_idx: usize,
) -> bool {
    let mut marker_idx = 0usize;
    while marker_idx < markers.len() {
        let marker = markers[marker_idx];
        if matches!(marker.event, ScopeEvent::Enter)
            && matches!(marker.scope_id.kind(), Some(ScopeKind::Parallel))
        {
            let left_arm = parallel_arm_at(markers, marker_idx, left_eff_idx);
            let right_arm = parallel_arm_at(markers, marker_idx, right_eff_idx);
            if matches!(
                (left_arm, right_arm),
                (Some(0), Some(1)) | (Some(1), Some(0))
            ) {
                return true;
            }
        }
        marker_idx += 1;
    }
    false
}

const fn on_endpoint_route_path(
    markers: &[ScopeMarker],
    candidate_eff_idx: usize,
    earlier_eff_idx: usize,
    later_eff_idx: usize,
) -> bool {
    let mut marker_idx = 0usize;
    while marker_idx < markers.len() {
        if is_first_route_enter(markers, marker_idx)
            && let Some(candidate_arm) = route_arm_at(markers, marker_idx, candidate_eff_idx)
        {
            let earlier_matches = match route_arm_at(markers, marker_idx, earlier_eff_idx) {
                Some(arm) => arm == candidate_arm,
                None => false,
            };
            let later_matches = match route_arm_at(markers, marker_idx, later_eff_idx) {
                Some(arm) => arm == candidate_arm,
                None => false,
            };
            if !earlier_matches && !later_matches {
                return false;
            }
        }
        marker_idx += 1;
    }
    true
}

const fn local_ordered(
    markers: &[ScopeMarker],
    earlier_eff_idx: usize,
    later_eff_idx: usize,
) -> bool {
    earlier_eff_idx < later_eff_idx
        && !different_parallel_arms(markers, earlier_eff_idx, later_eff_idx)
        && !mutually_exclusive_route_arms(markers, earlier_eff_idx, later_eff_idx)
}

/// Proves a causal handoff from the earlier receive to the later sender using
/// only projected local order and intervening send-to-receive edges. Route-arm
/// events may participate only when one endpoint fixes that arm; unrelated
/// branch-local traffic cannot become accidental ordering evidence.
const fn receive_precedes_later_send(
    eff_list: &EffList,
    earlier_eff_idx: usize,
    later_eff_idx: usize,
) -> bool {
    let markers = eff_list.scope_markers();
    let earlier = eff_list.node_at(earlier_eff_idx).atom_data();
    let mut witnessed_at = [ABSENT_WITNESS; crate::g::ROLE_DOMAIN_SIZE as usize];
    witnessed_at[earlier.to as usize] = earlier_eff_idx as u16;

    let mut eff_idx = earlier_eff_idx + 1;
    while eff_idx <= later_eff_idx {
        let node = eff_list.node_at(eff_idx);
        if matches!(node.kind, crate::eff::EffKind::Atom)
            && on_endpoint_route_path(markers, eff_idx, earlier_eff_idx, later_eff_idx)
        {
            let atom = node.atom_data();
            let witness = witnessed_at[atom.from as usize];
            if witness != ABSENT_WITNESS && local_ordered(markers, witness as usize, eff_idx) {
                if eff_idx == later_eff_idx {
                    return true;
                }
                if witnessed_at[atom.to as usize] == ABSENT_WITNESS {
                    witnessed_at[atom.to as usize] = eff_idx as u16;
                }
            }
        }
        eff_idx += 1;
    }
    false
}

/// A physical receive lane may change sender only after a descriptor-derived
/// causal handoff proves that the earlier frame was consumed, or across
/// mutually exclusive route arms. Parallel arms already use disjoint lanes.
pub(crate) const fn validate_receive_lane_causality(eff_list: &EffList) -> bool {
    let markers = eff_list.scope_markers();
    let mut left_idx = 0usize;
    while left_idx < eff_list.len() {
        let left_node = eff_list.node_at(left_idx);
        if matches!(left_node.kind, crate::eff::EffKind::Atom) {
            let left = left_node.atom_data();
            if left.from != left.to {
                let mut right_idx = left_idx + 1;
                while right_idx < eff_list.len() {
                    let right_node = eff_list.node_at(right_idx);
                    if matches!(right_node.kind, crate::eff::EffKind::Atom) {
                        let right = right_node.atom_data();
                        if right.from != right.to
                            && left.to == right.to
                            && left.lane == right.lane
                            && left.from != right.from
                            && !mutually_exclusive_route_arms(markers, left_idx, right_idx)
                            && !receive_precedes_later_send(eff_list, left_idx, right_idx)
                        {
                            return false;
                        }
                    }
                    right_idx += 1;
                }
            }
        }
        left_idx += 1;
    }
    true
}
