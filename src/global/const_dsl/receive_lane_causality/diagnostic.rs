use super::*;

/// Explain a rejected receive lane using the same must-analysis as validation.
/// This is diagnostic evidence only; it never authorizes projection.
pub(crate) const fn receive_lane_conflict<const E: usize>(
    eff_list: &EffList<E>,
) -> Option<(usize, usize, Option<u16>)> {
    let range = FlowRange {
        start: 0,
        end: eff_list.len(),
        marker_floor: 0,
    };
    let mut earlier_idx = 0;
    while earlier_idx < eff_list.len() {
        let earlier = eff_list.atom_at(earlier_idx);
        if earlier.from != earlier.to {
            let mut later_idx = earlier_idx + 1;
            while later_idx < eff_list.len() {
                let goal = FlowGoal::ReceiveLane(earlier, later_idx + 1);
                if goal.sender_change(eff_list.atom_at(later_idx)) {
                    let flow = CausalFlow {
                        eff_list,
                        earlier: earlier_idx,
                        goal,
                        body_start: 0,
                        iteration_start: 0,
                    };
                    if flow.advance(range, CausalRoles::empty()).is_none() {
                        return Some((earlier_idx, later_idx, None));
                    }
                }
                later_idx += 1;
            }
        }
        earlier_idx += 1;
    }
    let markers = eff_list.scope_markers();
    let mut marker_idx = 0;
    while marker_idx < markers.len() {
        let marker = markers.at(marker_idx);
        if marker.event.is_primary_enter()
            && matches!(marker.scope_id.kind(), Some(ScopeKind::Roll))
            && let Some((start, end)) = roll_body_range_from_enter(markers, marker_idx)
        {
            let range = FlowRange {
                start,
                end,
                marker_floor: 0,
            };
            let mut earlier_idx = start;
            while earlier_idx < end {
                let earlier = eff_list.atom_at(earlier_idx);
                if earlier.from != earlier.to {
                    let flow = CausalFlow {
                        eff_list,
                        earlier: earlier_idx - start,
                        goal: FlowGoal::Closure,
                        body_start: start,
                        iteration_start: 0,
                    };
                    if let Some(facts) = flow.advance(range, CausalRoles::empty()) {
                        let mut later_idx = start;
                        while later_idx < end {
                            let goal =
                                FlowGoal::ReceiveLane(earlier, end - start + later_idx + 1 - start);
                            if goal.sender_change(eff_list.atom_at(later_idx)) {
                                let next = CausalFlow {
                                    goal,
                                    iteration_start: end - start,
                                    ..flow
                                };
                                if next.advance(range, facts).is_none() {
                                    return Some((
                                        earlier_idx,
                                        later_idx,
                                        Some(marker.scope_id.local_ordinal()),
                                    ));
                                }
                            }
                            later_idx += 1;
                        }
                    }
                }
                earlier_idx += 1;
            }
        }
        marker_idx += 1;
    }
    None
}
