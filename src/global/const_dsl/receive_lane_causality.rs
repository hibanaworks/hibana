use super::event_relations::{events_are_locally_ordered, events_are_route_exclusive};
use super::scope_ranges::roll_body_range_from_enter;
use super::{EffList, ScopeEvent, ScopeKind, ScopeMarkerView, route_arm_ranges_from_first_enter};

const CAUSAL_ROLE_COUNT: usize = u8::MAX as usize + 1;
const NO_CAUSAL_WITNESS: u32 = u32::MAX;
const _: () =
    assert!(crate::eff::meta::COMPACT_EVENT_IDENTITY_CAPACITY * 2 < NO_CAUSAL_WITNESS as usize);

/// The causal closure retains only the first occurrence that transfers
/// authority to each role. Its storage follows the exact wire role domain and
/// is independent of choreography size.
struct FirstCausalWitnesses {
    by_role: [u32; CAUSAL_ROLE_COUNT],
}

impl FirstCausalWitnesses {
    #[inline(always)]
    const fn new(role: u8, occurrence_idx: usize) -> Self {
        let mut witnesses = Self {
            by_role: [NO_CAUSAL_WITNESS; CAUSAL_ROLE_COUNT],
        };
        witnesses.record_first(role, occurrence_idx);
        witnesses
    }

    #[inline(always)]
    const fn first(&self, role: u8) -> Option<usize> {
        let occurrence_idx = self.by_role[role as usize];
        if occurrence_idx == NO_CAUSAL_WITNESS {
            None
        } else {
            Some(occurrence_idx as usize)
        }
    }

    #[inline(always)]
    const fn record_first(&mut self, role: u8, occurrence_idx: usize) {
        let slot = &mut self.by_role[role as usize];
        if *slot != NO_CAUSAL_WITNESS {
            return;
        }
        if occurrence_idx >= NO_CAUSAL_WITNESS as usize {
            panic!("causality occurrence index exceeds the compact witness domain");
        }
        *slot = occurrence_idx as u32;
    }
}

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

    const fn is_valid_for<const E: usize>(self, eff_list: &EffList<E>) -> bool {
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

const fn is_first_route_enter(markers: ScopeMarkerView<'_>, marker_idx: usize) -> bool {
    let marker = markers.at(marker_idx);
    if !matches!(marker.event, ScopeEvent::Enter)
        || !matches!(marker.scope_id.kind(), Some(ScopeKind::Route))
    {
        return false;
    }
    let mut idx = 0usize;
    while idx < marker_idx {
        let candidate = markers.at(idx);
        if matches!(candidate.event, ScopeEvent::Enter) && candidate.scope_id.same(marker.scope_id)
        {
            return false;
        }
        idx += 1;
    }
    true
}

const fn route_arm_at(
    markers: ScopeMarkerView<'_>,
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

const fn on_endpoint_route_path(
    markers: ScopeMarkerView<'_>,
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
    markers: ScopeMarkerView<'_>,
    earlier_eff_idx: usize,
    later_eff_idx: usize,
) -> bool {
    events_are_locally_ordered(markers, earlier_eff_idx, later_eff_idx)
}

#[inline(always)]
const fn propagate_causal_witness(
    markers: ScopeMarkerView<'_>,
    witnesses: &mut FirstCausalWitnesses,
    eff_idx: usize,
    atom: crate::eff::EffAtom,
) -> bool {
    let Some(witness) = witnesses.first(atom.from) else {
        return false;
    };
    if !local_ordered(markers, witness, eff_idx) {
        return false;
    }
    witnesses.record_first(atom.to, eff_idx);
    true
}

/// Proves a causal handoff from the earlier receive to the later sender using
/// only projected local order and intervening send-to-receive edges. Route-arm
/// events may participate only when one endpoint fixes that arm; unrelated
/// branch-local traffic cannot become accidental ordering evidence.
const fn receive_precedes_later_send<const E: usize>(
    eff_list: &EffList<E>,
    earlier_eff_idx: usize,
    later_eff_idx: usize,
) -> bool {
    let markers = eff_list.scope_markers();
    let earlier_node = eff_list.node_at(earlier_eff_idx);
    if !matches!(earlier_node.kind, crate::eff::EffKind::Atom) {
        return false;
    }
    let mut witnesses = FirstCausalWitnesses::new(earlier_node.atom_data().to, earlier_eff_idx);

    let mut eff_idx = earlier_eff_idx + 1;
    while eff_idx <= later_eff_idx {
        let node = eff_list.node_at(eff_idx);
        if matches!(node.kind, crate::eff::EffKind::Atom)
            && on_endpoint_route_path(markers, eff_idx, earlier_eff_idx, later_eff_idx)
        {
            let atom = node.atom_data();
            if propagate_causal_witness(markers, &mut witnesses, eff_idx, atom)
                && eff_idx == later_eff_idx
            {
                return true;
            }
        }
        eff_idx += 1;
    }
    false
}

/// In a scope-free sequence the route-path predicate is constant for every
/// later endpoint. One forward closure per earlier receive therefore checks the
/// same pairs as repeated endpoint-specific closures without recomputing each
/// prefix.
const fn validate_linear_later_senders<const E: usize>(
    eff_list: &EffList<E>,
    earlier_eff_idx: usize,
) -> bool {
    let earlier_node = eff_list.node_at(earlier_eff_idx);
    if !matches!(earlier_node.kind, crate::eff::EffKind::Atom) {
        return false;
    }
    let earlier = earlier_node.atom_data();
    let markers = eff_list.scope_markers();
    if markers.len() != 0 {
        return false;
    }
    let mut witnesses = FirstCausalWitnesses::new(earlier.to, earlier_eff_idx);
    let mut eff_idx = earlier_eff_idx + 1;
    while eff_idx < eff_list.len() {
        let node = eff_list.node_at(eff_idx);
        if matches!(node.kind, crate::eff::EffKind::Atom) {
            let candidate = node.atom_data();
            let causally_preceded =
                propagate_causal_witness(markers, &mut witnesses, eff_idx, candidate);
            if candidate.from != candidate.to
                && earlier.to == candidate.to
                && earlier.lane == candidate.lane
                && earlier.from != candidate.from
                && !causally_preceded
            {
                return false;
            }
        }
        eff_idx += 1;
    }
    true
}

const fn validate_linear_receive_lane_causality<const E: usize>(eff_list: &EffList<E>) -> bool {
    let mut earlier_eff_idx = 0usize;
    while earlier_eff_idx < eff_list.len() {
        let node = eff_list.node_at(earlier_eff_idx);
        if matches!(node.kind, crate::eff::EffKind::Atom) {
            let earlier = node.atom_data();
            if earlier.from != earlier.to
                && !validate_linear_later_senders(eff_list, earlier_eff_idx)
            {
                return false;
            }
        }
        earlier_eff_idx += 1;
    }
    true
}

const fn route_reexecutes_in_roll_body(
    markers: ScopeMarkerView<'_>,
    route_enter_idx: usize,
    body: RollBodyRange,
) -> bool {
    let (_, left_start, _, _, _, right_end) =
        route_arm_ranges_from_first_enter(markers, route_enter_idx);
    body.start <= left_start && right_end <= body.end
}

const fn unfolded_route_path_contains(
    markers: ScopeMarkerView<'_>,
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
    markers: ScopeMarkerView<'_>,
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
    markers: ScopeMarkerView<'_>,
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
const fn receive_precedes_after_roll_reentry<const E: usize>(
    eff_list: &EffList<E>,
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

    let markers = eff_list.scope_markers();
    let earlier_node = eff_list.node_at(earlier_eff_idx);
    if !matches!(earlier_node.kind, crate::eff::EffKind::Atom) {
        return false;
    }
    let mut witnesses =
        FirstCausalWitnesses::new(earlier_node.atom_data().to, earlier_unfolded_idx);

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
            ) && let Some(witness) = witnesses.first(candidate.from)
                && unfolded_locally_ordered(markers, body, witness, unfolded_idx)
            {
                if unfolded_idx == later_unfolded_idx {
                    return true;
                }
                witnesses.record_first(candidate.to, unfolded_idx);
            }
        }
        unfolded_idx += 1;
    }
    false
}

const fn validate_roll_body_receive_lane_causality<const E: usize>(
    eff_list: &EffList<E>,
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

const fn validate_roll_receive_lane_causality<const E: usize>(eff_list: &EffList<E>) -> bool {
    let markers = eff_list.scope_markers();
    let mut marker_idx = 0usize;
    while marker_idx < markers.len() {
        let marker = markers.at(marker_idx);
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

const fn validate_structured_receive_lane_causality<const E: usize>(eff_list: &EffList<E>) -> bool {
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
                            && !events_are_route_exclusive(markers, left_idx, right_idx)
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

/// A physical receive lane may change sender only after a descriptor-derived
/// causal handoff proves that the earlier frame was consumed, or across
/// mutually exclusive route arms. Parallel arms already use disjoint lanes.
pub(crate) const fn validate_receive_lane_causality<const E: usize>(eff_list: &EffList<E>) -> bool {
    let markers = eff_list.scope_markers();
    let receive_lanes_are_safe = if markers.len() == 0 {
        validate_linear_receive_lane_causality(eff_list)
    } else {
        validate_structured_receive_lane_causality(eff_list)
    };
    receive_lanes_are_safe && validate_roll_receive_lane_causality(eff_list)
}

#[cfg(kani)]
mod kani;
