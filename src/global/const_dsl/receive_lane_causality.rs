use super::scope_ranges::roll_body_range_from_enter;
use super::{EffList, ScopeEvent, ScopeKind, scope::ScopeEntry};

mod actors;
mod directions;
mod participants;
pub(crate) use participants::Participants;
mod relations;
pub(crate) use actors::owned_receive_lane_conflict;
pub(crate) use actors::receive_pair_is_causal;
use actors::{ActorMap, NeedChoice};
pub(crate) use actors::{ReceiveScan, receive_lane_conflict_in};
pub(crate) use directions::LogicalDirections;
pub(crate) use directions::has_receive_sender_changes;
use directions::{Direction, ReceiveDirectionIndex};
pub(crate) use relations::{ReceiveOrder, ReceiveRelations, ReceiveSeeds};

const CAUSAL_ROLE_COUNT: usize = u8::MAX as usize + 1;
const CAUSAL_ROLE_WORDS: usize = CAUSAL_ROLE_COUNT / u64::BITS as usize;

/// Compile-time must facts, indexed by the exact actor domain. A fact
/// means the earlier receive precedes this actor's next local action. Branches
/// share their incoming facts, never another parallel arm's outgoing facts.
#[derive(Clone, Copy)]
pub(crate) struct CausalRoles([u64; CAUSAL_ROLE_WORDS]);

impl CausalRoles {
    const fn empty() -> Self {
        Self([0; CAUSAL_ROLE_WORDS])
    }

    const fn contains(&self, role: u8) -> bool {
        self.0[role as usize / 64] & (1u64 << (role as usize % 64)) != 0
    }

    const fn is_empty(&self) -> bool {
        self.0[0] | self.0[1] | self.0[2] | self.0[3] == 0
    }

    const fn insert(&mut self, role: u8) {
        self.0[role as usize / 64] |= 1u64 << (role as usize % 64);
    }

    const fn is_subset_of(&self, other: &Self) -> bool {
        self.0[0] & !other.0[0] == 0
            && self.0[1] & !other.0[1] == 0
            && self.0[2] & !other.0[2] == 0
            && self.0[3] & !other.0[3] == 0
    }

    pub(crate) const fn is_disjoint(self, other: Self) -> bool {
        self.0[0] & other.0[0] == 0
            && self.0[1] & other.0[1] == 0
            && self.0[2] & other.0[2] == 0
            && self.0[3] & other.0[3] == 0
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

    #[cfg(any(kani, all(test, hibana_repo_tests)))]
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

/// Scope entries and possible participants, shared by source causal queries.
/// Lane recoloring preserves both, so the index is built once per source.
pub(crate) struct FlowIndex<const E: usize> {
    first: [u16; E],
    first_receive: [u16; CAUSAL_ROLE_COUNT],
    next_receive: [u16; E],
    reentry_end: [u16; E],
    enclosing: [u16; E],
    scopes: [FlowScope; E],
    marker_len: usize,
    participants: participants::Participants<E>,
}

#[derive(Clone, Copy)]
struct FlowScope {
    next: u16,
    start: u16,
    split: u16,
    end: u16,
    kind: ScopeKind,
    parent: u16,
    roll: u16,
    reentry_end: u16,
}

impl<const E: usize> FlowIndex<E> {
    pub(crate) const fn participants_at(&self, event: usize) -> CausalRoles {
        self.participants.at(event)
    }
    pub(crate) const fn lane_participants(
        &self,
        events: &EffList<E>,
        lane: u8,
        output: &mut Participants<E>,
    ) {
        output.for_lane(events, &self.participants, lane);
    }
    const fn scope_at(&self, range: &mut FlowRange) -> Option<(usize, ScopeKind, usize, usize)> {
        let scopes = self;
        if range.marker_floor > scopes.marker_len {
            crate::invariant();
        }
        let first = self.first[range.start] as usize;
        let mut idx = if range.marker_floor > first {
            range.marker_floor
        } else {
            first
        };
        while idx < scopes.marker_len {
            idx = scopes.scopes[idx].next as usize;
            if idx == scopes.marker_len {
                break;
            }
            let scope = scopes.scopes[idx];
            let offset = scope.start as usize;
            if offset < range.start {
                crate::invariant();
            }
            if offset > range.start {
                break;
            }
            if scope.end as usize <= range.end {
                range.marker_floor = idx;
                return Some((idx, scope.kind, scope.split as usize, scope.end as usize));
            }
            idx += 1;
        }
        range.marker_floor = idx;
        None
    }
    pub(crate) const fn new<const D: usize>(
        events: &EffList<E>,
        actors: Option<&crate::g::SourceActorImage<E, D>>,
    ) -> Self {
        let markers = events.scope_markers();
        if markers.len() > u16::MAX as usize || events.len() > u16::MAX as usize {
            crate::invariant();
        }
        let mut result = Self {
            first: [0; E],
            first_receive: [u16::MAX; CAUSAL_ROLE_COUNT],
            next_receive: [u16::MAX; E],
            reentry_end: [u16::MAX; E],
            enclosing: [u16::MAX; E],
            scopes: [FlowScope {
                next: markers.len() as u16,
                start: 0,
                split: 0,
                end: 0,
                kind: ScopeKind::Roll,
                parent: u16::MAX,
                roll: u16::MAX,
                reentry_end: u16::MAX,
            }; E],
            marker_len: markers.len(),
            participants: participants::Participants::new(events, actors),
        };
        let mut event = events.len();
        while event != 0 {
            event -= 1;
            let atom = events.atom_at(event);
            if atom.from != atom.to {
                result.next_receive[event] = result.first_receive[atom.to as usize];
                result.first_receive[atom.to as usize] = event as u16;
            }
        }
        let mut marker = markers.len();
        let mut next = markers.len() as u16;
        while marker != 0 {
            marker -= 1;
            let entry = markers.at(marker);
            if entry.event.is_primary_enter() {
                next = marker as u16;
                let (kind, split, end) = match (entry.event, entry.scope_id.kind()) {
                    (
                        ScopeEvent::Enter(ScopeEntry::Route { right_end }),
                        Some(ScopeKind::Route),
                    ) => (ScopeKind::Route, entry.segment_end(), right_end as usize),
                    (
                        ScopeEvent::Enter(ScopeEntry::Parallel { split }),
                        Some(ScopeKind::Parallel),
                    ) => (ScopeKind::Parallel, split as usize, entry.segment_end()),
                    (ScopeEvent::Enter(ScopeEntry::Roll), Some(ScopeKind::Roll)) => {
                        (ScopeKind::Roll, entry.segment_end(), entry.segment_end())
                    }
                    _ => crate::invariant(),
                };
                result.scopes[marker] = FlowScope {
                    next,
                    start: entry.offset() as u16,
                    split: split as u16,
                    end: end as u16,
                    kind,
                    parent: u16::MAX,
                    roll: u16::MAX,
                    reentry_end: u16::MAX,
                };
            } else {
                result.scopes[marker].next = next;
            }
        }
        let mut marker = 0;
        let mut event = 0;
        let mut ancestors = [u16::MAX; E];
        let mut depth = 0;
        while event < events.len() {
            while marker < markers.len() && markers.at(marker).offset() < event {
                marker += 1;
            }
            result.first[event] = marker as u16;
            while depth != 0 && result.scopes[ancestors[depth - 1] as usize].end as usize <= event {
                depth -= 1;
            }
            let inherited = depth;
            let mut entry = marker;
            while entry < markers.len() && markers.at(entry).offset() == event {
                let scope = result.scopes[entry];
                if markers.at(entry).event.is_primary_enter() && scope.end as usize > event {
                    if inherited != 0
                        && scope.end > result.scopes[ancestors[inherited - 1] as usize].end
                    {
                        crate::invariant();
                    }
                    // Equal-offset markers may be emitted in another ID order.
                    // Link their enclosing intervals from outermost to innermost.
                    let mut place = depth;
                    while place > inherited
                        && scope.end > result.scopes[ancestors[place - 1] as usize].end
                    {
                        ancestors[place] = ancestors[place - 1];
                        place -= 1;
                    }
                    ancestors[place] = entry as u16;
                    depth += 1;
                }
                entry += 1;
            }
            let mut level = inherited;
            while level < depth {
                let entry = ancestors[level] as usize;
                let parent = if level == 0 {
                    u16::MAX
                } else {
                    ancestors[level - 1]
                };
                result.scopes[entry].parent = parent;
                result.scopes[entry].roll = if matches!(result.scopes[entry].kind, ScopeKind::Roll)
                {
                    entry as u16
                } else if parent == u16::MAX {
                    u16::MAX
                } else {
                    result.scopes[parent as usize].roll
                };
                result.scopes[entry].reentry_end =
                    if matches!(result.scopes[entry].kind, ScopeKind::Roll) {
                        result.scopes[entry].end
                    } else if parent == u16::MAX {
                        u16::MAX
                    } else {
                        result.scopes[parent as usize].reentry_end
                    };
                level += 1;
            }
            if depth != 0 {
                let inner = ancestors[depth - 1];
                result.enclosing[event] = inner;
                result.reentry_end[event] = result.scopes[inner as usize].reentry_end;
            }
            event += 1;
        }
        result
    }

    pub(crate) const fn enclosing_scope(&self, event: usize) -> u16 {
        self.enclosing[event]
    }

    pub(crate) const fn parent_scope(&self, scope: u16) -> u16 {
        self.scopes[scope as usize].parent
    }

    const fn enclosing_roll(&self, event: usize) -> u16 {
        let scope = self.enclosing[event];
        if scope == u16::MAX {
            u16::MAX
        } else {
            self.scopes[scope as usize].roll
        }
    }

    const fn parent_roll(&self, scope: u16) -> u16 {
        let parent = self.scopes[scope as usize].parent;
        if parent == u16::MAX {
            u16::MAX
        } else {
            self.scopes[parent as usize].roll
        }
    }

    pub(crate) const fn parallel_sibling(
        &self,
        scope: u16,
        start: usize,
        end: usize,
    ) -> Option<(usize, usize)> {
        let scope = self.scopes[scope as usize];
        if !matches!(scope.kind, ScopeKind::Parallel) {
            return None;
        }
        if scope.start as usize <= start && end <= scope.split as usize {
            Some((scope.split as usize, scope.end as usize))
        } else if scope.split as usize <= start && end <= scope.end as usize {
            Some((scope.start as usize, scope.split as usize))
        } else {
            None
        }
    }

    /// Nonlocal receives at this logical role, in source order. Lane
    /// recoloring changes neither the receiver nor membership in this list.
    pub(crate) const fn first_receive_to(&self, receiver: u8) -> u16 {
        self.first_receive[receiver as usize]
    }

    pub(crate) const fn next_receive_to(&self, event: u16) -> u16 {
        self.next_receive[event as usize]
    }

    /// A shared source scope separates the events in the same visit.
    /// Choices are exclusive; parallel arms share no outgoing causal facts.
    const fn separating_scope(&self, earlier: usize, later: usize) -> Option<ScopeKind> {
        let mut index = self.enclosing[earlier];
        while index != u16::MAX {
            let scope = self.scopes[index as usize];
            if scope.start as usize <= later
                && later < scope.end as usize
                && (earlier < scope.split as usize) != (later < scope.split as usize)
            {
                return Some(scope.kind);
            }
            index = scope.parent;
        }
        None
    }

    /// A fresh visit can coexist with this roll's following continuation.
    /// Single-visit causal relays do not order that fresh receive against it.
    const fn reentry_can_cross(&self, first: usize, second: usize) -> bool {
        let (body, continuation) = if first < second {
            (first, second)
        } else {
            (second, first)
        };
        self.reentry_end[body] as usize <= continuation
    }

    const fn first_reentry_conflict(
        &self,
        events: &EffList<E>,
        earlier: usize,
        stop: usize,
    ) -> Option<usize> {
        let cutoff = self.reentry_end[earlier] as usize;
        if cutoff >= stop {
            return None;
        }
        let before = events.atom_at(earlier);
        let mut next = self.first_receive_to(before.to);
        while next != u16::MAX && (next as usize) < stop {
            if next as usize >= cutoff {
                let after = events.atom_at(next as usize);
                if after.from != before.from && after.lane == before.lane {
                    return Some(next as usize);
                }
            }
            next = self.next_receive_to(next);
        }
        None
    }
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
    scope_offsets: &'a FlowIndex<E>,
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

    const fn first_receive_target(&self, range: FlowRange) -> Option<usize> {
        match self.goal {
            FlowGoal::ReceiveEvent(_, target) => {
                if !self.contains(range, target) {
                    return None;
                }
                let event = self.body_start + target - self.iteration_start;
                if self
                    .goal
                    .sender_change(target, self.eff_list.atom_at(event))
                {
                    Some(event)
                } else {
                    None
                }
            }
            FlowGoal::ReceiveLane(_, stop) => {
                let mut event = range.start;
                while event < range.end && self.occurrence(event) < stop {
                    if self
                        .goal
                        .sender_change(self.occurrence(event), self.eff_list.atom_at(event))
                    {
                        return Some(event);
                    }
                    event += 1;
                }
                None
            }
            #[cfg(any(kani, all(test, hibana_repo_tests)))]
            FlowGoal::Target(_) => None,
            FlowGoal::Closure => None,
        }
    }

    #[cfg(any(kani, all(test, hibana_repo_tests)))]
    const fn advance(&self, range: FlowRange, facts: CausalRoles) -> Option<CausalRoles> {
        match self.advance_as(range, facts, &ActorMap::<E, 0>::Logical) {
            Ok(Ok(facts)) => Some(facts),
            Ok(Err(_)) => None,
            Err(_) => crate::invariant(),
        }
    }

    const fn advance_as<const D: usize>(
        &self,
        mut range: FlowRange,
        mut facts: CausalRoles,
        actors: &ActorMap<'_, E, D>,
    ) -> Result<Result<CausalRoles, usize>, NeedChoice> {
        let stop = self.goal.stop();
        let selected_target = self.goal.selected_target();
        if self.occurrence(range.end) <= self.earlier || self.occurrence(range.start) >= stop {
            return Ok(Ok(facts));
        }
        // Convert this query's cutoff once. Every nested range uses source
        // indices; only reported failures need occurrence coordinates.
        let source_stop = if stop == usize::MAX {
            usize::MAX
        } else {
            self.body_start + (stop - self.iteration_start)
        };
        // Every possible participant in this prefix already knows the seed.
        // Its relays and branch joins preserve these facts, and each receive
        // target has a known sender. The interval index shares this check
        // across queries rather than scanning the same subtree again.
        let prefix_end = if range.end < source_stop {
            range.end
        } else {
            source_stop
        };
        if !facts.is_empty()
            && self
                .scope_offsets
                .participants
                .covered(range.start, prefix_end, &facts)
        {
            return Ok(Ok(facts));
        }
        // An independent arm cannot relay a fact that it never received.
        // If it contains neither the seed nor a target, its whole subtree
        // contributes the empty set, regardless of its choices and scopes.
        if facts.is_empty()
            && !self.contains(range, self.earlier)
            && self.first_receive_target(range).is_none()
        {
            return Ok(Ok(facts));
        }
        while range.start < range.end && range.start < source_stop {
            // Relays cannot create evidence from an empty input. Before the
            // seed, seek directly to it or to the next structural boundary;
            // a containing route/par must still fork using its shared input.
            if self.occurrence(range.start) < self.earlier && facts.is_empty() {
                let seed = self.body_start + self.earlier - self.iteration_start;
                let scopes = self.scope_offsets;
                let first = self.scope_offsets.first[range.start] as usize;
                let next = if range.marker_floor > first {
                    range.marker_floor
                } else {
                    first
                };
                let next = if next < scopes.marker_len {
                    scopes.scopes[next].next as usize
                } else {
                    next
                };
                let boundary =
                    if next < scopes.marker_len && (scopes.scopes[next].start as usize) < seed {
                        scopes.scopes[next].start as usize
                    } else {
                        seed
                    };
                if boundary > range.start {
                    range.start = boundary;
                }
            }
            if let Some((idx, kind, left_end, right_end)) = self.scope_offsets.scope_at(&mut range)
            {
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
                                Ok(Ok(value)) => value,
                                Ok(Err(event)) => return Ok(Err(event)),
                                Err(choice) => return Err(choice),
                            };
                            let right = match self.advance_as(right, facts, actors) {
                                Ok(Ok(value)) => value,
                                Ok(Err(event)) => return Ok(Err(event)),
                                Err(choice) => return Err(choice),
                            };
                            Ok(Ok(match kind {
                                ScopeKind::Route => left.intersect(right),
                                ScopeKind::Parallel => left.union(right),
                                ScopeKind::Roll => crate::invariant(),
                            }))
                        }
                    }
                };
                let joined = match advanced {
                    Ok(Ok(value)) => value,
                    Ok(Err(event)) => return Ok(Err(event)),
                    Err(choice) => return Err(choice),
                };
                facts = joined;
                // If the target was inside the scope, its prefix is complete.
                if right_end > source_stop {
                    return Ok(Ok(facts));
                }
                range.start = right_end;
            } else {
                // Scope lookup is unchanged until the next source marker.
                // Run the intervening sequential events without seeking again.
                let scopes = self.scope_offsets;
                let boundary = if range.marker_floor < scopes.marker_len {
                    let next = scopes.scopes[range.marker_floor].start as usize;
                    if next < range.end { next } else { range.end }
                } else {
                    range.end
                };
                if boundary <= range.start {
                    crate::invariant();
                }
                // Within a marker-free block, an empty input cannot relay
                // knowledge before a seed. Seek the first obligation directly;
                // retain its ordinary actor/path check at the target itself.
                let block = FlowRange {
                    end: if boundary < source_stop {
                        boundary
                    } else {
                        source_stop
                    },
                    ..range
                };
                if facts.is_empty() && !self.contains(block, self.earlier) {
                    match self.first_receive_target(block) {
                        Some(event) => range.start = event,
                        None => {
                            range.start = block.end;
                            continue;
                        }
                    }
                }
                let mut occurrence = self.occurrence(range.start);
                while range.start < boundary && range.start < source_stop {
                    let atom = self.eff_list.atom_at(range.start);
                    let receiver = if occurrence == self.earlier {
                        match actors.receiver(range.start, atom, self.iteration_start) {
                            Ok(value) => value,
                            Err(choice) => return Err(choice),
                        }
                    } else {
                        let target =
                            occurrence > self.earlier && self.goal.sender_change(occurrence, atom);
                        let receiver = actors.receiver(range.start, atom, self.iteration_start);
                        // A non-target relay cannot add evidence to a receiver
                        // that already has it, whatever its sender's owner is.
                        if !target && matches!(&receiver, Ok(receiver) if facts.contains(*receiver))
                        {
                            range.start += 1;
                            occurrence += 1;
                            continue;
                        }
                        let known_sender = match actors.sender_known(
                            range.start,
                            atom,
                            self.iteration_start,
                            &facts,
                        ) {
                            Ok(value) => value,
                            Err(choice) => return Err(choice),
                        };
                        if target && !known_sender {
                            return Ok(Err(occurrence));
                        }
                        if !known_sender {
                            range.start += 1;
                            occurrence += 1;
                            continue;
                        }
                        match receiver {
                            Ok(value) => value,
                            Err(choice) => return Err(choice),
                        }
                    };
                    facts.insert(receiver);
                    range.start += 1;
                    occurrence += 1;
                }
            }
        }
        Ok(Ok(facts))
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
#[cfg(all(test, hibana_repo_tests))]
const fn receive_precedes_later_send<const E: usize>(
    eff_list: &EffList<E>,
    earlier_eff_idx: usize,
    later_eff_idx: usize,
) -> bool {
    let scope_offsets = FlowIndex::new::<0>(eff_list, None);
    let flow = CausalFlow {
        eff_list,
        scope_offsets: &scope_offsets,
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
#[cfg(any(kani, all(test, hibana_repo_tests)))]
const fn validate_linear_later_senders<const E: usize>(
    eff_list: &EffList<E>,
    earlier_eff_idx: usize,
    end: usize,
) -> bool {
    let earlier = eff_list.atom_at(earlier_eff_idx);
    let mut facts = CausalRoles::empty();
    facts.insert(earlier.to);
    let mut eff_idx = earlier_eff_idx + 1;
    while eff_idx < end {
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

#[cfg(any(kani, all(test, hibana_repo_tests)))]
const fn validate_linear_receive_lane_causality<const E: usize>(eff_list: &EffList<E>) -> bool {
    let mut storage = [Direction::EMPTY; E];
    let directions = ReceiveDirectionIndex::new(eff_list, &mut storage);
    let mut earlier_eff_idx = 0usize;
    while earlier_eff_idx < eff_list.len() {
        let earlier = eff_list.atom_at(earlier_eff_idx);
        if earlier.from != earlier.to
            && let Some(end) =
                directions.last_change(earlier, (earlier_eff_idx + 1, eff_list.len()))
            && !validate_linear_later_senders(eff_list, earlier_eff_idx, end)
        {
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
    let scope_offsets = FlowIndex::new::<0>(eff_list, None);
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
        scope_offsets: &scope_offsets,
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
#[cfg(any(kani, all(test, hibana_repo_tests)))]
const fn validate_roll_body_receive_lane_causality<const E: usize>(
    eff_list: &EffList<E>,
    body: RollBodyRange,
) -> bool {
    let scope_offsets = FlowIndex::new::<0>(eff_list, None);
    if !body.is_valid_for(eff_list) {
        return false;
    }
    let range = FlowRange {
        start: body.start,
        end: body.end,
        marker_floor: 0,
    };
    let mut earlier_idx = body.start;
    let mut storage = [Direction::EMPTY; E];
    let directions = ReceiveDirectionIndex::new(eff_list, &mut storage);
    while earlier_idx < body.end {
        let earlier = eff_list.atom_at(earlier_idx);
        if earlier.from != earlier.to {
            let Some(end) = directions.last_change(earlier, (body.start, body.end)) else {
                earlier_idx += 1;
                continue;
            };
            let flow = CausalFlow {
                eff_list,
                scope_offsets: &scope_offsets,
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

#[cfg(any(kani, all(test, hibana_repo_tests)))]
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

#[cfg(any(kani, all(test, hibana_repo_tests)))]
const fn validate_structured_receive_lane_causality<const E: usize>(eff_list: &EffList<E>) -> bool {
    let scope_offsets = FlowIndex::new::<0>(eff_list, None);
    let mut storage = [Direction::EMPTY; E];
    let directions = ReceiveDirectionIndex::new(eff_list, &mut storage);
    let range = FlowRange {
        start: 0,
        end: eff_list.len(),
        marker_floor: 0,
    };
    let mut earlier_idx = 0usize;
    while earlier_idx < eff_list.len() {
        let earlier = eff_list.atom_at(earlier_idx);
        if earlier.from != earlier.to {
            let Some(end) = directions.last_change(earlier, (earlier_idx + 1, eff_list.len()))
            else {
                earlier_idx += 1;
                continue;
            };
            if scope_offsets
                .first_reentry_conflict(eff_list, earlier_idx, end)
                .is_some()
            {
                return false;
            }
            let flow = CausalFlow {
                eff_list,
                scope_offsets: &scope_offsets,
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
#[cfg(any(kani, all(test, hibana_repo_tests)))]
pub(crate) const fn validate_receive_lane_causality<const E: usize>(eff_list: &EffList<E>) -> bool {
    let markers = eff_list.scope_markers();
    let receive_lanes_are_safe = if markers.len() == 0 {
        validate_linear_receive_lane_causality(eff_list)
    } else {
        validate_structured_receive_lane_causality(eff_list)
    };
    receive_lanes_are_safe && validate_roll_receive_lane_causality(eff_list)
}

/// Find the same first rejected pair used by allocation, without rebuilding
/// a separate per-target causal search for the diagnostic.
pub(crate) const fn receive_lane_conflict<const E: usize>(
    events: &EffList<E>,
) -> Option<(usize, usize, Option<u16>)> {
    let index = FlowIndex::new::<0>(events, None);
    receive_lane_conflict_in::<E, 0>(events, &index, None, ReceiveScan::From(None), None)
}

#[cfg(kani)]
mod kani;

#[cfg(all(test, hibana_repo_tests))]
mod tests;
