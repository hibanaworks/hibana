use super::scope_ranges::roll_body_range_from_enter;
use super::{
    EffList, ScopeKind, parallel_arm_ranges_from_enter, route_arm_ranges_from_first_enter,
};

const CAUSAL_ROLE_COUNT: usize = u8::MAX as usize + 1;
const CAUSAL_ROLE_WORDS: usize = CAUSAL_ROLE_COUNT / u64::BITS as usize;

/// Compile-time must facts, indexed by the exact wire role domain. A fact
/// means the earlier receive precedes this role's next local action. Branches
/// share their incoming facts, never another parallel arm's outgoing facts.
#[derive(Clone, Copy)]
struct CausalRoles([u64; CAUSAL_ROLE_WORDS]);

impl CausalRoles {
    const fn empty() -> Self {
        Self([0; CAUSAL_ROLE_WORDS])
    }

    const fn contains(self, role: u8) -> bool {
        self.0[role as usize / 64] & (1u64 << (role as usize % 64)) != 0
    }

    const fn is_empty(self) -> bool {
        let mut word = 0;
        while word < CAUSAL_ROLE_WORDS {
            if self.0[word] != 0 {
                return false;
            }
            word += 1;
        }
        true
    }

    const fn insert(&mut self, role: u8) {
        self.0[role as usize / 64] |= 1u64 << (role as usize % 64);
    }

    const fn intersect(self, other: Self) -> Self {
        let mut result = Self::empty();
        let mut word = 0;
        while word < CAUSAL_ROLE_WORDS {
            result.0[word] = self.0[word] & other.0[word];
            word += 1;
        }
        result
    }

    const fn union(self, other: Self) -> Self {
        let mut result = Self::empty();
        let mut word = 0;
        while word < CAUSAL_ROLE_WORDS {
            result.0[word] = self.0[word] | other.0[word];
            word += 1;
        }
        result
    }

    const fn handoff(&mut self, atom: crate::eff::EffAtom) {
        if self.contains(atom.from) {
            self.insert(atom.to);
        }
    }
}

/// A source range and its preorder marker floor identify one structured arm;
/// nested scopes with identical event ranges still have distinct floors.
#[derive(Clone, Copy)]
struct FlowRange {
    start: usize,
    end: usize,
    marker_floor: usize,
}

#[derive(Clone, Copy)]
enum FlowGoal {
    #[cfg(any(kani, all(test, hibana_repo_tests)))]
    Target(usize),
    ReceiveLane(crate::eff::EffAtom, usize),
    Closure,
}

impl FlowGoal {
    const fn stop(self) -> usize {
        match self {
            #[cfg(any(kani, all(test, hibana_repo_tests)))]
            Self::Target(index) => index,
            Self::ReceiveLane(_, end) => end,
            Self::Closure => usize::MAX,
        }
    }

    // A bulk check has several targets. Its cutoff must never select only
    // the arm containing the last one; every earlier obligation is checked.
    const fn selected_target(self) -> usize {
        match self {
            #[cfg(any(kani, all(test, hibana_repo_tests)))]
            Self::Target(index) => index,
            Self::ReceiveLane(_, _) | Self::Closure => usize::MAX,
        }
    }

    const fn sender_change(self, candidate: crate::eff::EffAtom) -> bool {
        match self {
            Self::ReceiveLane(earlier, _) => {
                candidate.from != candidate.to
                    && earlier.to == candidate.to
                    && earlier.lane == candidate.lane
                    && earlier.from != candidate.from
            }
            #[cfg(any(kani, all(test, hibana_repo_tests)))]
            Self::Target(_) => false,
            Self::Closure => false,
        }
    }
}

struct CausalFlow<'a, const E: usize> {
    eff_list: &'a EffList<E>,
    earlier: usize,
    goal: FlowGoal,
    body_start: usize,
    iteration_start: usize,
}

impl<const E: usize> CausalFlow<'_, E> {
    const fn occurrence(&self, eff_idx: usize) -> usize {
        self.iteration_start + eff_idx - self.body_start
    }

    const fn contains(&self, range: FlowRange, occurrence: usize) -> bool {
        self.occurrence(range.start) <= occurrence && occurrence < self.occurrence(range.end)
    }

    const fn scope_at(&self, range: &mut FlowRange) -> Option<usize> {
        let markers = self.eff_list.scope_markers();
        let mut idx = markers.offset_lower_bound(range.start, range.marker_floor);
        while idx < markers.len() {
            let marker = markers.at(idx);
            if marker.offset() > range.start {
                break;
            }
            if marker.offset() == range.start && marker.event.is_primary_enter() {
                let end = match marker.scope_id.kind() {
                    Some(ScopeKind::Route) => {
                        let [_, (_, end)] = route_arm_ranges_from_first_enter(markers, idx);
                        end
                    }
                    Some(ScopeKind::Parallel | ScopeKind::Roll) => marker.segment_end(),
                    None => crate::invariant(),
                };
                if end <= range.end {
                    range.marker_floor = idx;
                    return Some(idx);
                }
            }
            idx += 1;
        }
        range.marker_floor = idx;
        None
    }

    const fn advance(&self, mut range: FlowRange, mut facts: CausalRoles) -> Option<CausalRoles> {
        if self.occurrence(range.end) <= self.earlier {
            return Some(facts);
        }
        while range.start < range.end && self.occurrence(range.start) < self.goal.stop() {
            // Relays cannot create evidence from an empty input. Before the
            // seed, seek directly to it or to the next structural boundary;
            // a containing route/par must still fork using its shared input.
            if self.occurrence(range.start) < self.earlier && facts.is_empty() {
                let seed = self.body_start + self.earlier - self.iteration_start;
                let markers = self.eff_list.scope_markers();
                let next = markers.offset_lower_bound(range.start, range.marker_floor);
                let boundary = if next < markers.len() && markers.at(next).offset() < seed {
                    markers.at(next).offset()
                } else {
                    seed
                };
                if boundary > range.start {
                    range.start = boundary;
                }
            }
            if let Some(idx) = self.scope_at(&mut range) {
                let markers = self.eff_list.scope_markers();
                let marker = markers.at(idx);
                let (left_end, right_end) = match marker.scope_id.kind() {
                    Some(ScopeKind::Route) => {
                        let [(_, split), (_, end)] =
                            route_arm_ranges_from_first_enter(markers, idx);
                        (split, end)
                    }
                    Some(ScopeKind::Parallel) => {
                        let Some((_, split, _, end)) = parallel_arm_ranges_from_enter(markers, idx)
                        else {
                            crate::invariant()
                        };
                        (split, end)
                    }
                    Some(ScopeKind::Roll) => (marker.segment_end(), marker.segment_end()),
                    None => crate::invariant(),
                };
                let left = FlowRange {
                    start: range.start,
                    end: left_end,
                    marker_floor: idx + 1,
                };
                let right = FlowRange {
                    start: left_end,
                    end: right_end,
                    marker_floor: idx + 1,
                };
                let advanced = match marker.scope_id.kind() {
                    Some(ScopeKind::Roll) => self.advance(left, facts),
                    Some(kind @ (ScopeKind::Route | ScopeKind::Parallel)) => {
                        let selected = if self.contains(left, self.goal.selected_target())
                            || matches!(kind, ScopeKind::Route) && self.contains(left, self.earlier)
                        {
                            Some(left)
                        } else if self.contains(right, self.goal.selected_target())
                            || matches!(kind, ScopeKind::Route)
                                && self.contains(right, self.earlier)
                        {
                            Some(right)
                        } else {
                            None
                        };
                        if let Some(arm) = selected {
                            self.advance(arm, facts)
                        } else {
                            // Fork with a shared input; join only after each arm
                            // completes. One recursive frame per source scope.
                            let Some(left) = self.advance(left, facts) else {
                                return None;
                            };
                            let Some(right) = self.advance(right, facts) else {
                                return None;
                            };
                            Some(match kind {
                                ScopeKind::Route => left.intersect(right),
                                ScopeKind::Parallel => left.union(right),
                                ScopeKind::Roll => crate::invariant(),
                            })
                        }
                    }
                    None => crate::invariant(),
                };
                let Some(joined) = advanced else { return None };
                facts = joined;
                // If the target was inside the scope, its prefix is complete.
                if self.occurrence(right_end) > self.goal.stop() {
                    return Some(facts);
                }
                range.start = right_end;
            } else {
                let atom = self.eff_list.atom_at(range.start);
                if self.occurrence(range.start) == self.earlier {
                    facts.insert(atom.to);
                } else {
                    if self.occurrence(range.start) > self.earlier
                        && self.goal.sender_change(atom)
                        && !facts.contains(atom.from)
                    {
                        return None;
                    }
                    facts.handoff(atom);
                }
                range.start += 1;
            }
        }
        Some(facts)
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

    #[cfg(any(kani, all(test, hibana_repo_tests)))]
    const fn contains(self, eff_idx: usize) -> bool {
        self.start <= eff_idx && eff_idx < self.end
    }

    const fn is_valid_for<const E: usize>(self, eff_list: &EffList<E>) -> bool {
        self.start < self.end && self.end <= eff_list.len()
    }
}

/// Structured must analysis: sequence composes facts, route intersects them,
/// and parallel forks share only their input. No concrete arm occurrence is
/// fabricated as a witness for a fact established by every route arm.
#[cfg(any(kani, all(test, hibana_repo_tests)))]
const fn receive_precedes_later_send<const E: usize>(
    eff_list: &EffList<E>,
    earlier_eff_idx: usize,
    later_eff_idx: usize,
) -> bool {
    let flow = CausalFlow {
        eff_list,
        earlier: earlier_eff_idx,
        goal: FlowGoal::Target(later_eff_idx),
        body_start: 0,
        iteration_start: 0,
    };
    let Some(facts) = flow.advance(
        FlowRange {
            start: 0,
            end: eff_list.len(),
            marker_floor: 0,
        },
        CausalRoles::empty(),
    ) else {
        crate::invariant()
    };
    facts.contains(eff_list.atom_at(later_eff_idx).from)
}

/// A sequence has no fork contexts, so one forward closure checks all later
/// senders for an earlier receive without recomputing their common prefixes.
const fn validate_linear_later_senders<const E: usize>(
    eff_list: &EffList<E>,
    earlier_eff_idx: usize,
) -> bool {
    let earlier = eff_list.atom_at(earlier_eff_idx);
    let mut facts = CausalRoles::empty();
    facts.insert(earlier.to);
    let mut eff_idx = earlier_eff_idx + 1;
    while eff_idx < eff_list.len() {
        let candidate = eff_list.atom_at(eff_idx);
        if candidate.from != candidate.to
            && earlier.to == candidate.to
            && earlier.lane == candidate.lane
            && earlier.from != candidate.from
            && !facts.contains(candidate.from)
        {
            return false;
        }
        facts.handoff(candidate);
        eff_idx += 1;
    }
    true
}

const fn validate_linear_receive_lane_causality<const E: usize>(eff_list: &EffList<E>) -> bool {
    let mut earlier_eff_idx = 0usize;
    while earlier_eff_idx < eff_list.len() {
        let earlier = eff_list.atom_at(earlier_eff_idx);
        if earlier.from != earlier.to && !validate_linear_later_senders(eff_list, earlier_eff_idx) {
            return false;
        }
        earlier_eff_idx += 1;
    }
    true
}

/// Two visits use the same descriptors and independent route choices. Only
/// facts at the end of the current iteration enter the next iteration; source
/// rows and runtime state are never copied or expanded.
#[cfg(any(kani, all(test, hibana_repo_tests)))]
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
    let range = FlowRange {
        start: body.start,
        end: body.end,
        marker_floor: 0,
    };
    let flow = CausalFlow {
        eff_list,
        earlier: earlier_eff_idx - body.start,
        goal: FlowGoal::Target(body.len() + later_eff_idx - body.start),
        body_start: body.start,
        iteration_start: 0,
    };
    let Some(facts) = flow.advance(range, CausalRoles::empty()) else {
        crate::invariant()
    };
    let next = CausalFlow {
        iteration_start: body.len(),
        ..flow
    };
    let Some(facts) = next.advance(range, facts) else {
        crate::invariant()
    };
    facts.contains(eff_list.atom_at(later_eff_idx).from)
}

/// The last sender change bounds all obligations for this receive. No later
/// scope or event can affect an earlier handoff; do not evaluate that suffix.
/// This also identifies sender-stable lanes without allocating lane tables.
const fn sender_change_end<const E: usize>(
    eff_list: &EffList<E>,
    range: FlowRange,
    earlier: crate::eff::EffAtom,
) -> Option<usize> {
    let goal = FlowGoal::ReceiveLane(earlier, range.end);
    let mut end = None;
    let mut index = range.start;
    while index < range.end {
        if goal.sender_change(eff_list.atom_at(index)) {
            end = Some(index + 1);
        }
        index += 1;
    }
    end
}

const fn validate_roll_body_receive_lane_causality<const E: usize>(
    eff_list: &EffList<E>,
    body: RollBodyRange,
) -> bool {
    if !body.is_valid_for(eff_list) {
        return false;
    }
    let range = FlowRange {
        start: body.start,
        end: body.end,
        marker_floor: 0,
    };
    let mut earlier_idx = body.start;
    while earlier_idx < body.end {
        let earlier = eff_list.atom_at(earlier_idx);
        if earlier.from != earlier.to {
            let Some(end) = sender_change_end(eff_list, range, earlier) else {
                earlier_idx += 1;
                continue;
            };
            let flow = CausalFlow {
                eff_list,
                earlier: earlier_idx - body.start,
                goal: FlowGoal::Closure,
                body_start: body.start,
                iteration_start: 0,
            };
            let Some(facts) = flow.advance(range, CausalRoles::empty()) else {
                crate::invariant()
            };
            let next = CausalFlow {
                goal: FlowGoal::ReceiveLane(earlier, body.len() + end - body.start),
                iteration_start: body.len(),
                ..flow
            };
            if next.advance(range, facts).is_none() {
                return false;
            }
        }
        earlier_idx += 1;
    }
    true
}

const fn validate_roll_receive_lane_causality<const E: usize>(eff_list: &EffList<E>) -> bool {
    let markers = eff_list.scope_markers();
    let mut marker_idx = 0usize;
    while marker_idx < markers.len() {
        let marker = markers.at(marker_idx);
        if marker.event.is_primary_enter()
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
    let range = FlowRange {
        start: 0,
        end: eff_list.len(),
        marker_floor: 0,
    };
    let mut earlier_idx = 0usize;
    while earlier_idx < eff_list.len() {
        let earlier = eff_list.atom_at(earlier_idx);
        if earlier.from != earlier.to {
            let Some(end) = sender_change_end(
                eff_list,
                FlowRange {
                    start: earlier_idx + 1,
                    ..range
                },
                earlier,
            ) else {
                earlier_idx += 1;
                continue;
            };
            let flow = CausalFlow {
                eff_list,
                earlier: earlier_idx,
                goal: FlowGoal::ReceiveLane(earlier, end),
                body_start: 0,
                iteration_start: 0,
            };
            if flow.advance(range, CausalRoles::empty()).is_none() {
                return false;
            }
        }
        earlier_idx += 1;
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

mod diagnostic;
pub(crate) use diagnostic::receive_lane_conflict;

#[cfg(kani)]
mod kani;

#[cfg(all(test, hibana_repo_tests))]
mod tests;
