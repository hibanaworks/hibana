use super::scope_ranges::roll_body_range_from_enter;
use super::{EffList, ScopeEvent, ScopeKind, scope::ScopeEntry};

mod actors;
mod directions;
pub(crate) use actors::owned_receive_lane_conflict;
pub(crate) use actors::owned_receive_lane_conflict_from;
pub(crate) use actors::receive_pair_is_causal;
use actors::{ActorMap, NeedChoice};
use directions::ReceiveDirectionIndex;

const CAUSAL_ROLE_COUNT: usize = u8::MAX as usize + 1;
const CAUSAL_ROLE_WORDS: usize = CAUSAL_ROLE_COUNT / u64::BITS as usize;

/// Compile-time must facts, indexed by the exact actor domain. A fact
/// means the earlier receive precedes this actor's next local action. Branches
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
        self.0[0] | self.0[1] | self.0[2] | self.0[3] == 0
    }

    const fn insert(&mut self, role: u8) {
        self.0[role as usize / 64] |= 1u64 << (role as usize % 64);
    }

    const fn intersect(self, other: Self) -> Self {
        Self([
            self.0[0] & other.0[0],
            self.0[1] & other.0[1],
            self.0[2] & other.0[2],
            self.0[3] & other.0[3],
        ])
    }

    const fn union(self, other: Self) -> Self {
        Self([
            self.0[0] | other.0[0],
            self.0[1] | other.0[1],
            self.0[2] | other.0[2],
            self.0[3] | other.0[3],
        ])
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
    ReceiveEvent(crate::eff::EffAtom, usize),
    Closure,
}

impl FlowGoal {
    const fn stop(self) -> usize {
        match self {
            #[cfg(any(kani, all(test, hibana_repo_tests)))]
            Self::Target(index) => index,
            Self::ReceiveLane(_, end) => end,
            Self::ReceiveEvent(_, event) => event + 1,
            Self::Closure => usize::MAX,
        }
    }

    // A bulk check has several targets. Its cutoff must never select only
    // the arm containing the last one; every earlier obligation is checked.
    const fn selected_target(self) -> Option<usize> {
        match self {
            #[cfg(any(kani, all(test, hibana_repo_tests)))]
            Self::Target(index) => Some(index),
            Self::ReceiveLane(_, _) | Self::ReceiveEvent(_, _) | Self::Closure => None,
        }
    }

    const fn sender_change(self, occurrence: usize, candidate: crate::eff::EffAtom) -> bool {
        match self {
            Self::ReceiveLane(earlier, _) => {
                candidate.from != candidate.to
                    && earlier.to == candidate.to
                    && earlier.lane == candidate.lane
                    && earlier.from != candidate.from
            }
            Self::ReceiveEvent(earlier, event) => {
                occurrence == event
                    && candidate.from != candidate.to
                    && earlier.to == candidate.to
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

    const fn has_receive_target(&self, range: FlowRange) -> bool {
        match self.goal {
            FlowGoal::ReceiveEvent(_, target) => {
                if !self.contains(range, target) {
                    return false;
                }
                let event = self.body_start + target - self.iteration_start;
                self.goal
                    .sender_change(target, self.eff_list.atom_at(event))
            }
            FlowGoal::ReceiveLane(_, stop) => {
                let mut event = range.start;
                while event < range.end && self.occurrence(event) < stop {
                    if self
                        .goal
                        .sender_change(self.occurrence(event), self.eff_list.atom_at(event))
                    {
                        return true;
                    }
                    event += 1;
                }
                false
            }
            #[cfg(any(kani, all(test, hibana_repo_tests)))]
            FlowGoal::Target(_) => false,
            FlowGoal::Closure => false,
        }
    }

    const fn scope_at(&self, range: &mut FlowRange) -> Option<(usize, ScopeKind, usize, usize)> {
        let markers = self.eff_list.scope_markers();
        if range.marker_floor > markers.len() {
            crate::invariant();
        }
        let mut idx = range.marker_floor;
        while idx < markers.len() {
            let marker = markers.at(idx);
            let offset = marker.offset();
            if offset < range.start {
                idx = markers.offset_lower_bound(range.start, idx + 1);
                continue;
            }
            if offset > range.start {
                break;
            }
            if marker.event.is_primary_enter() {
                let (kind, left_end, end) = match (marker.event, marker.scope_id.kind()) {
                    (
                        ScopeEvent::Enter(ScopeEntry::Route { right_end }),
                        Some(ScopeKind::Route),
                    ) => (ScopeKind::Route, marker.segment_end(), right_end as usize),
                    (
                        ScopeEvent::Enter(ScopeEntry::Parallel { split }),
                        Some(ScopeKind::Parallel),
                    ) => (ScopeKind::Parallel, split as usize, marker.segment_end()),
                    (ScopeEvent::Enter(ScopeEntry::Roll), Some(ScopeKind::Roll)) => {
                        (ScopeKind::Roll, marker.segment_end(), marker.segment_end())
                    }
                    _ => crate::invariant(),
                };
                if end <= range.end {
                    range.marker_floor = idx;
                    return Some((idx, kind, left_end, end));
                }
            }
            idx += 1;
        }
        range.marker_floor = idx;
        None
    }

    const fn advance(&self, range: FlowRange, facts: CausalRoles) -> Option<CausalRoles> {
        match self.advance_as(range, facts, ActorMap::<E, 0>::Logical) {
            Ok(result) => result,
            Err(_) => crate::invariant(),
        }
    }

    const fn advance_as<const D: usize>(
        &self,
        mut range: FlowRange,
        mut facts: CausalRoles,
        actors: ActorMap<'_, E, D>,
    ) -> Result<Option<CausalRoles>, NeedChoice> {
        let stop = self.goal.stop();
        let selected_target = self.goal.selected_target();
        if self.occurrence(range.end) <= self.earlier || self.occurrence(range.start) >= stop {
            return Ok(Some(facts));
        }
        // Every event in a covered range preserves these facts: its sender
        // already knows the seed, and its receiver adds no new fact. Sequence,
        // choice and parallel composition therefore all return the same set.
        // Check both endpoints, including unselected arms, before skipping it.
        // A single event already needs only one relay check.
        if range.start + 1 < range.end && !facts.is_empty() {
            let mut event = range.start;
            let mut covered = true;
            while event < range.end {
                let atom = self.eff_list.atom_at(event);
                if !matches!(
                    actors.sender_known(event, atom, self.iteration_start, facts),
                    Ok(true)
                ) {
                    covered = false;
                    break;
                }
                match actors.receiver(event, atom, self.iteration_start) {
                    Ok(receiver) if facts.contains(receiver) => {}
                    _ => {
                        covered = false;
                        break;
                    }
                }
                event += 1;
            }
            if covered {
                return Ok(Some(facts));
            }
        }
        // An independent arm cannot relay a fact that it never received.
        // If it contains neither the seed nor a target, its whole subtree
        // contributes the empty set, regardless of its choices and scopes.
        if facts.is_empty()
            && !self.contains(range, self.earlier)
            && !self.has_receive_target(range)
        {
            return Ok(Some(facts));
        }
        while range.start < range.end && self.occurrence(range.start) < stop {
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
            if let Some((idx, kind, left_end, right_end)) = self.scope_at(&mut range) {
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
                let advanced = match kind {
                    ScopeKind::Roll => self.advance_as(left, facts, actors),
                    kind @ (ScopeKind::Route | ScopeKind::Parallel) => {
                        let known_arm = if matches!(kind, ScopeKind::Route) {
                            actors.selected_route(
                                range.start,
                                left_end,
                                right_end,
                                self.iteration_start,
                            )
                        } else {
                            None
                        };
                        let selected = if let Some(arm) = known_arm {
                            Some(match arm {
                                crate::g::SourceArm::Left => left,
                                crate::g::SourceArm::Right => right,
                            })
                        } else if matches!(selected_target, Some(target) if self.contains(left, target))
                            || matches!(kind, ScopeKind::Route) && self.contains(left, self.earlier)
                        {
                            Some(left)
                        } else if matches!(selected_target, Some(target) if self.contains(right, target))
                            || matches!(kind, ScopeKind::Route)
                                && self.contains(right, self.earlier)
                        {
                            Some(right)
                        } else {
                            None
                        };
                        if let Some(arm) = selected {
                            self.advance_as(arm, facts, actors)
                        } else {
                            // Fork with a shared input; join only after each arm
                            // completes. One recursive frame per source scope.
                            let left = match self.advance_as(left, facts, actors) {
                                Ok(Some(value)) => value,
                                Ok(None) => return Ok(None),
                                Err(choice) => return Err(choice),
                            };
                            let right = match self.advance_as(right, facts, actors) {
                                Ok(Some(value)) => value,
                                Ok(None) => return Ok(None),
                                Err(choice) => return Err(choice),
                            };
                            Ok(Some(match kind {
                                ScopeKind::Route => left.intersect(right),
                                ScopeKind::Parallel => left.union(right),
                                ScopeKind::Roll => crate::invariant(),
                            }))
                        }
                    }
                };
                let joined = match advanced {
                    Ok(Some(value)) => value,
                    Ok(None) => return Ok(None),
                    Err(choice) => return Err(choice),
                };
                facts = joined;
                // If the target was inside the scope, its prefix is complete.
                if self.occurrence(right_end) > stop {
                    return Ok(Some(facts));
                }
                range.start = right_end;
            } else {
                // Scope lookup is unchanged until the next source marker.
                // Run the intervening sequential events without seeking again.
                let markers = self.eff_list.scope_markers();
                let boundary = if range.marker_floor < markers.len() {
                    let next = markers.at(range.marker_floor).offset();
                    if next < range.end { next } else { range.end }
                } else {
                    range.end
                };
                if boundary <= range.start {
                    crate::invariant();
                }
                while range.start < boundary && self.occurrence(range.start) < stop {
                    let atom = self.eff_list.atom_at(range.start);
                    let occurrence = self.occurrence(range.start);
                    if occurrence == self.earlier {
                        let receiver =
                            match actors.receiver(range.start, atom, self.iteration_start) {
                                Ok(value) => value,
                                Err(choice) => return Err(choice),
                            };
                        facts.insert(receiver);
                    } else {
                        let known_sender = match actors.sender_known(
                            range.start,
                            atom,
                            self.iteration_start,
                            facts,
                        ) {
                            Ok(value) => value,
                            Err(choice) => return Err(choice),
                        };
                        if occurrence > self.earlier
                            && self.goal.sender_change(occurrence, atom)
                            && !known_sender
                        {
                            return Ok(None);
                        }
                        if known_sender {
                            let receiver =
                                match actors.receiver(range.start, atom, self.iteration_start) {
                                    Ok(value) => value,
                                    Err(choice) => return Err(choice),
                                };
                            facts.insert(receiver);
                        }
                    }
                    range.start += 1;
                }
            }
        }
        Ok(Some(facts))
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
    let directions = ReceiveDirectionIndex::new(eff_list, body.start, body.end);
    while earlier_idx < body.end {
        let earlier = eff_list.atom_at(earlier_idx);
        if earlier.from != earlier.to {
            let Some(end) = directions.last_change(earlier, body.start) else {
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
    let directions = ReceiveDirectionIndex::new(eff_list, 0, eff_list.len());
    let range = FlowRange {
        start: 0,
        end: eff_list.len(),
        marker_floor: 0,
    };
    let mut earlier_idx = 0usize;
    while earlier_idx < eff_list.len() {
        let earlier = eff_list.atom_at(earlier_idx);
        if earlier.from != earlier.to {
            let Some(end) = directions.last_change(earlier, earlier_idx + 1) else {
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
