use super::scope_ranges::roll_body_range_from_enter;
use super::{
    EffList, ScopeEvent, ScopeKind, ScopeMarker, parallel_arm_ranges_from_enter,
    route_arm_ranges_from_first_enter,
};

const ABSENT_WITNESS: u16 = u16::MAX;

#[derive(Clone, Copy)]
struct RollBodyRange {
    start: usize,
    end: usize,
}

impl RollBodyRange {
    const fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }

    const fn len(self) -> usize {
        self.end - self.start
    }

    const fn contains(self, eff_idx: usize) -> bool {
        self.start <= eff_idx && eff_idx < self.end
    }

    const fn is_valid_for(self, eff_list: &EffList) -> bool {
        self.start < self.end && self.end <= eff_list.len()
    }

    const fn unfolded_occurrence(self, unfolded_idx: usize) -> UnfoldedOccurrence {
        let len = self.len();
        UnfoldedOccurrence {
            eff_idx: self.start + unfolded_idx % len,
            iteration: if unfolded_idx < len {
                RollIteration::Current
            } else {
                RollIteration::Next
            },
        }
    }
}

#[derive(Clone, Copy)]
enum RollIteration {
    Current,
    Next,
}

impl RollIteration {
    const fn same(self, other: Self) -> bool {
        matches!(
            (self, other),
            (Self::Current, Self::Current) | (Self::Next, Self::Next)
        )
    }
}

#[derive(Clone, Copy)]
struct UnfoldedOccurrence {
    eff_idx: usize,
    iteration: RollIteration,
}

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

const fn route_reexecutes_in_roll_body(
    markers: &[ScopeMarker],
    route_enter_idx: usize,
    body: RollBodyRange,
) -> bool {
    let (_, left_start, _, _, _, right_end) =
        route_arm_ranges_from_first_enter(markers, route_enter_idx);
    body.start <= left_start && right_end <= body.end
}

const fn unfolded_route_path_contains(
    markers: &[ScopeMarker],
    route_enter_idx: usize,
    candidate: UnfoldedOccurrence,
    endpoint: UnfoldedOccurrence,
    body: RollBodyRange,
) -> bool {
    let Some(candidate_arm) = route_arm_at(markers, route_enter_idx, candidate.eff_idx) else {
        return false;
    };
    if route_reexecutes_in_roll_body(markers, route_enter_idx, body)
        && !candidate.iteration.same(endpoint.iteration)
    {
        return false;
    }
    matches!(
        route_arm_at(markers, route_enter_idx, endpoint.eff_idx),
        Some(endpoint_arm) if endpoint_arm == candidate_arm
    )
}

const fn on_unfolded_endpoint_route_path(
    markers: &[ScopeMarker],
    candidate: UnfoldedOccurrence,
    earlier: UnfoldedOccurrence,
    later: UnfoldedOccurrence,
    body: RollBodyRange,
) -> bool {
    let mut marker_idx = 0usize;
    while marker_idx < markers.len() {
        if is_first_route_enter(markers, marker_idx)
            && route_arm_at(markers, marker_idx, candidate.eff_idx).is_some()
            && !unfolded_route_path_contains(markers, marker_idx, candidate, earlier, body)
            && !unfolded_route_path_contains(markers, marker_idx, candidate, later, body)
        {
            return false;
        }
        marker_idx += 1;
    }
    true
}

const fn unfolded_locally_ordered(
    markers: &[ScopeMarker],
    body: RollBodyRange,
    earlier_unfolded_idx: usize,
    later_unfolded_idx: usize,
) -> bool {
    if earlier_unfolded_idx >= later_unfolded_idx {
        return false;
    }
    let earlier = body.unfolded_occurrence(earlier_unfolded_idx);
    let later = body.unfolded_occurrence(later_unfolded_idx);
    if !earlier.iteration.same(later.iteration) {
        return true;
    }
    local_ordered(markers, earlier.eff_idx, later.eff_idx)
}

/// Checks a sender-authority transfer across one explicit unfolding of a roll
/// body without copying its descriptor rows. Route identities inside the body
/// are iteration-local; enclosing route authority remains stable.
const fn receive_precedes_after_roll_reentry(
    eff_list: &EffList,
    body: RollBodyRange,
    earlier_eff_idx: usize,
    later_eff_idx: usize,
) -> bool {
    if !body.is_valid_for(eff_list)
        || !body.contains(earlier_eff_idx)
        || !body.contains(later_eff_idx)
    {
        return false;
    }
    let body_len = body.len();
    let earlier_unfolded_idx = earlier_eff_idx - body.start;
    let later_unfolded_idx = body_len + later_eff_idx - body.start;
    if later_unfolded_idx >= ABSENT_WITNESS as usize {
        return false;
    }

    let markers = eff_list.scope_markers();
    let earlier = eff_list.node_at(earlier_eff_idx).atom_data();
    let mut witnessed_at = [ABSENT_WITNESS; crate::g::ROLE_DOMAIN_SIZE as usize];
    witnessed_at[earlier.to as usize] = earlier_unfolded_idx as u16;

    let mut unfolded_idx = earlier_unfolded_idx + 1;
    while unfolded_idx <= later_unfolded_idx {
        let candidate_occurrence = body.unfolded_occurrence(unfolded_idx);
        let node = eff_list.node_at(candidate_occurrence.eff_idx);
        if matches!(node.kind, crate::eff::EffKind::Atom) {
            let candidate = node.atom_data();
            if on_unfolded_endpoint_route_path(
                markers,
                candidate_occurrence,
                UnfoldedOccurrence {
                    eff_idx: earlier_eff_idx,
                    iteration: RollIteration::Current,
                },
                UnfoldedOccurrence {
                    eff_idx: later_eff_idx,
                    iteration: RollIteration::Next,
                },
                body,
            ) {
                let witness = witnessed_at[candidate.from as usize];
                if witness != ABSENT_WITNESS
                    && unfolded_locally_ordered(markers, body, witness as usize, unfolded_idx)
                {
                    if unfolded_idx == later_unfolded_idx {
                        return true;
                    }
                    if witnessed_at[candidate.to as usize] == ABSENT_WITNESS {
                        witnessed_at[candidate.to as usize] = unfolded_idx as u16;
                    }
                }
            }
        }
        unfolded_idx += 1;
    }
    false
}

const fn validate_roll_body_receive_lane_causality(
    eff_list: &EffList,
    body: RollBodyRange,
) -> bool {
    if !body.is_valid_for(eff_list) {
        return false;
    }
    let mut left_idx = body.start;
    while left_idx < body.end {
        let left_node = eff_list.node_at(left_idx);
        if matches!(left_node.kind, crate::eff::EffKind::Atom) {
            let left = left_node.atom_data();
            if left.from != left.to {
                let mut right_idx = body.start;
                while right_idx < body.end {
                    let right_node = eff_list.node_at(right_idx);
                    if matches!(right_node.kind, crate::eff::EffKind::Atom) {
                        let right = right_node.atom_data();
                        if right.from != right.to
                            && left.to == right.to
                            && left.lane == right.lane
                            && left.from != right.from
                            && !receive_precedes_after_roll_reentry(
                                eff_list, body, left_idx, right_idx,
                            )
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

const fn validate_roll_receive_lane_causality(eff_list: &EffList) -> bool {
    let markers = eff_list.scope_markers();
    let mut marker_idx = 0usize;
    while marker_idx < markers.len() {
        let marker = markers[marker_idx];
        if matches!(marker.event, ScopeEvent::Enter)
            && matches!(marker.scope_id.kind(), Some(ScopeKind::Roll))
        {
            let Some((body_start, body_end)) = roll_body_range_from_enter(markers, marker_idx)
            else {
                return false;
            };
            if !validate_roll_body_receive_lane_causality(
                eff_list,
                RollBodyRange::new(body_start, body_end),
            ) {
                return false;
            }
        }
        marker_idx += 1;
    }
    true
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
    validate_roll_receive_lane_causality(eff_list)
}

#[cfg(kani)]
mod kani;
