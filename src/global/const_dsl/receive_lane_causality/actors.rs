//! Receive-order obligations use logical peers and lanes. Causal relays use
//! the physical actor holding each endpoint at that source event. Both come
//! from the same checked source; possible-holder sets are never authority.
use super::{
    CausalFlow, CausalRoles, Direction, EffList, FlowGoal, FlowIndex, FlowRange,
    ReceiveDirectionIndex, ReceiveOrder, RollBodyRange, ScopeKind, roll_body_range_from_enter,
};
use crate::{
    eff::EffAtom,
    g::{SourceActorImage, SourceArm, SourceContext, SourceScope, SourceSelection},
};

#[cfg(all(test, hibana_repo_tests, not(miri)))]
mod exhaustive;
#[cfg(any(kani, all(test, hibana_repo_tests, not(miri))))]
mod fixtures;
#[cfg(kani)]
mod kani;

/// Choices inside a repeated body are independent between its two visits.
/// Choices outside that body identify the same enclosing execution in both.
#[derive(Clone, Copy)]
enum Visit {
    Shared,
    Current,
    Next,
}

#[derive(Clone, Copy)]
pub(super) struct Paths<'a> {
    shared: &'a SourceContext<'a>,
    current: &'a SourceContext<'a>,
    next: &'a SourceContext<'a>,
}

impl<'a> Paths<'a> {
    const fn get(self, visit: Visit) -> &'a SourceContext<'a> {
        match visit {
            Visit::Shared => self.shared,
            Visit::Current => self.current,
            Visit::Next => self.next,
        }
    }
}

#[derive(Clone, Copy)]
pub(super) struct NeedChoice {
    scope: SourceScope,
    visit: Visit,
}

pub(super) enum ActorMap<'a, const E: usize, const D: usize> {
    Logical,
    Owned {
        image: &'a SourceActorImage<E, D>,
        events: &'a [crate::g::ActorPair],
        paths: Paths<'a>,
        repeated: Option<RollBodyRange>,
    },
}

impl<const E: usize, const D: usize> ActorMap<'_, E, D> {
    const fn visit(&self, scope: SourceScope, iteration: usize) -> Visit {
        if let Self::Owned {
            repeated: Some(body),
            ..
        } = self
            && scope.start as usize >= body.start
            && scope.stop as usize <= body.end
        {
            if iteration == 0 {
                Visit::Current
            } else {
                Visit::Next
            }
        } else {
            Visit::Shared
        }
    }

    const fn selection(&self, scope: SourceScope, iteration: usize) -> Option<SourceArm> {
        match self {
            Self::Logical => None,
            Self::Owned { paths, .. } => paths.get(self.visit(scope, iteration)).get(scope),
        }
    }

    pub(super) const fn selected_route(
        &self,
        start: usize,
        split: usize,
        stop: usize,
        iteration: usize,
    ) -> Option<SourceArm> {
        self.selection(
            SourceScope {
                start: start as u16,
                split: split as u16,
                stop: stop as u16,
            },
            iteration,
        )
    }

    const fn decision(&self, reference: u16) -> (SourceScope, [u16; 2]) {
        let Self::Owned { image, .. } = self else {
            crate::invariant()
        };
        match image.decision(reference as usize - 256) {
            Some(value) => value,
            None => crate::invariant(),
        }
    }

    pub(super) const fn receiver(
        &self,
        event: usize,
        atom: EffAtom,
        iteration: usize,
    ) -> Result<u8, NeedChoice> {
        let Self::Owned { events, .. } = self else {
            return Ok(atom.to);
        };
        let mut reference = events[event].receiver.0;
        while reference >= 256 {
            let (scope, children) = self.decision(reference);
            reference = match self.selection(scope, iteration) {
                Some(SourceArm::Left) => children[0],
                Some(SourceArm::Right) => children[1],
                None => {
                    return Err(NeedChoice {
                        scope,
                        visit: self.visit(scope, iteration),
                    });
                }
            };
        }
        Ok(reference as u8)
    }

    /// A known fact for every remaining holder needs no choice. Missing facts
    /// require a path-specific restart: a route intersection may have hidden
    /// the correlation between that route's relay and the holder it selected.
    const fn reference_known(
        &self,
        reference: u16,
        iteration: usize,
        facts: &CausalRoles,
    ) -> Result<bool, NeedChoice> {
        if reference < 256 {
            return Ok(facts.contains(reference as u8));
        }
        let (scope, children) = self.decision(reference);
        match self.selection(scope, iteration) {
            Some(SourceArm::Left) => self.reference_known(children[0], iteration, facts),
            Some(SourceArm::Right) => self.reference_known(children[1], iteration, facts),
            None => {
                let left = self.reference_known(children[0], iteration, facts);
                let right = self.reference_known(children[1], iteration, facts);
                match (left, right) {
                    (Ok(true), Ok(true)) => Ok(true),
                    _ => Err(NeedChoice {
                        scope,
                        visit: self.visit(scope, iteration),
                    }),
                }
            }
        }
    }

    pub(super) const fn sender_known(
        &self,
        event: usize,
        atom: EffAtom,
        iteration: usize,
        facts: &CausalRoles,
    ) -> Result<bool, NeedChoice> {
        match self {
            Self::Logical => Ok(facts.contains(atom.from)),
            Self::Owned { events, .. } => {
                let sender = events[event].sender.0;
                if sender < 256 {
                    Ok(facts.contains(sender as u8))
                } else {
                    self.reference_known(sender, iteration, facts)
                }
            }
        }
    }
}

struct Obligation<'a, const E: usize, const D: usize> {
    events: &'a EffList<E>,
    scope_offsets: &'a FlowIndex<E>,
    actors: Option<&'a SourceActorImage<E, D>>,
    earlier: usize,
    end: usize,
    repeated: Option<RollBodyRange>,
    passive_entry: Option<usize>,
}

impl<const E: usize, const D: usize> Obligation<'_, E, D> {
    const fn range(&self) -> RollBodyRange {
        match self.repeated {
            Some(body) => body,
            None => RollBodyRange::new(0, self.events.len()),
        }
    }

    const fn conflict_with_order(
        &self,
        target: Option<usize>,
        order: Option<ReceiveOrder<'_>>,
    ) -> Option<usize> {
        let context = SourceContext::new(&[]);
        let paths = Paths {
            shared: &context,
            current: &context,
            next: &context,
        };
        let body = self.range();
        let order = match (self.repeated, order) {
            (Some(_), Some(order)) if order.covers_roll(self.earlier, body.start, body.end) => {
                order
            }
            _ => return self.conflict(paths, target),
        };
        let before = self.events.atom_at(self.earlier);
        let goal = match target {
            Some(event) => FlowGoal::ReceiveEvent(before, event),
            None => FlowGoal::ReceiveLane(before, self.end),
        };
        let mut later = match target {
            Some(event) => event,
            None => body.start,
        };
        while later < self.end {
            if goal.sender_change(later, self.events.atom_at(later))
                && !order.repeated_contains(self.earlier, later)
            {
                return Some(later);
            }
            if target.is_some() {
                break;
            }
            later += 1;
        }
        None
    }

    const fn run(
        &self,
        actors: &ActorMap<'_, E, D>,
        target: Option<usize>,
        start: usize,
    ) -> Result<Option<usize>, NeedChoice> {
        let earlier = self.events.atom_at(self.earlier);
        let body = self.range();
        let mut choice_entry = None;
        if let Some(entry) = self.passive_entry {
            let later = match target {
                Some(later) => later,
                None => crate::invariant(),
            };
            let actor = match actors.receiver(self.earlier, earlier, 0) {
                Ok(actor) => actor,
                Err(choice) => return Err(choice),
            };
            let next_actor = match actors.receiver(later, self.events.atom_at(later), body.len()) {
                Ok(actor) => actor,
                Err(choice) => return Err(choice),
            };
            if actor != next_actor {
                return Ok(None);
            }
            let mut local = CausalRoles::empty();
            local.insert(actor);
            let current_send = match actors.sender_known(self.earlier, earlier, 0, &local) {
                Ok(value) => value,
                Err(choice) => return Err(choice),
            };
            let next_send =
                match actors.sender_known(later, self.events.atom_at(later), body.len(), &local) {
                    Ok(value) => value,
                    Err(choice) => return Err(choice),
                };
            // Co-resident logical views have a local send before their
            // receive row. The actor chooses that action itself; it is not
            // an unseen first incoming branch from another actor.
            if current_send || next_send {
                return Ok(None);
            }
            let scope = self.scope_offsets.scopes[entry];
            let current_arm = if self.earlier < scope.split as usize {
                FlowRange {
                    start: scope.start as usize,
                    end: scope.split as usize,
                    marker_floor: entry + 1,
                }
            } else {
                FlowRange {
                    start: scope.split as usize,
                    end: scope.end as usize,
                    marker_floor: entry + 1,
                }
            };
            let next_arm = if later < scope.split as usize {
                FlowRange {
                    start: scope.start as usize,
                    end: scope.split as usize,
                    marker_floor: entry + 1,
                }
            } else {
                FlowRange {
                    start: scope.split as usize,
                    end: scope.end as usize,
                    marker_floor: entry + 1,
                }
            };
            // A passive actor chooses its first arm from the incoming frame.
            // A prior local action in that arm instead fixes its continuation.
            let current = match self.actor_precedes(actors, current_arm, self.earlier, actor, 0) {
                Ok(value) => value,
                Err(choice) => return Err(choice),
            };
            let next = match self.actor_precedes(actors, next_arm, later, actor, body.len()) {
                Ok(value) => value,
                Err(choice) => return Err(choice),
            };
            if current || next {
                return Ok(None);
            }
            choice_entry = Some((current_arm.start - body.start, entry + 1, actor));
            let fenced = match self.fifo_fence(
                actors,
                FlowRange {
                    start: body.start,
                    end: body.end,
                    marker_floor: 0,
                },
                self.events.atom_at(later),
                false,
            ) {
                Ok((_, fenced)) => fenced,
                Err(choice) => return Err(choice),
            };
            if fenced {
                return Ok(None);
            }
        }
        let range = FlowRange {
            start: body.start,
            end: body.end,
            marker_floor: 0,
        };
        let flow = CausalFlow {
            eff_list: self.events,
            scope_offsets: self.scope_offsets,
            earlier: self.earlier - body.start,
            goal: match self.repeated {
                Some(_) => FlowGoal::Closure,
                None => match target {
                    Some(event) => FlowGoal::ReceiveEvent(earlier, event),
                    None => FlowGoal::ReceiveLane(earlier, self.end),
                },
            },
            body_start: body.start,
            iteration_start: 0,
            choice_entry,
        };
        let facts =
            match flow.advance_as(FlowRange { start, ..range }, CausalRoles::empty(), actors) {
                Ok(Ok(value)) => value,
                Ok(Err(event)) => return Ok(Some(event)),
                Err(choice) => return Err(choice),
            };
        if self.repeated.is_none() {
            return Ok(None);
        }
        // A fact known after the complete current visit stays known through
        // every next-visit prefix. No partial branch output is used here.
        if let Some(event) = target
            && matches!(
                actors.sender_known(event, self.events.atom_at(event), body.len(), &facts),
                Ok(true)
            )
        {
            return Ok(None);
        }
        let next = CausalFlow {
            goal: match target {
                Some(event) => FlowGoal::ReceiveEvent(earlier, body.len() + event - body.start),
                None => FlowGoal::ReceiveLane(earlier, body.len() + self.end - body.start),
            },
            iteration_start: body.len(),
            choice_entry: None,
            ..flow
        };
        match next.advance_as(range, facts, actors) {
            Ok(Ok(_)) => Ok(None),
            Ok(Err(event)) => Ok(Some(body.start + event - body.len())),
            Err(choice) => Err(choice),
        }
    }

    /// A mandatory receive in the current arm or after it, on the next
    /// entry's exact wire direction, fences that later frame. Consuming a
    /// sibling's guarded input also identifies the old arm before reentry.
    /// Parallel arms share their input; an unfixed route requires both arms.
    const fn fifo_fence(
        &self,
        actors: &ActorMap<'_, E, D>,
        mut range: FlowRange,
        entry: EffAtom,
        mut seen: bool,
    ) -> Result<(bool, bool), NeedChoice> {
        if let Some(index) = self.passive_entry {
            let scope = self.scope_offsets.scopes[index];
            let start = if self.earlier < scope.split as usize {
                scope.start as usize
            } else {
                scope.split as usize
            };
            if range.start == start && range.marker_floor == index + 1 {
                seen = true;
            }
        }
        while range.start < range.end {
            if let Some((index, kind, split, end)) = self.scope_offsets.scope_at(&mut range) {
                let left = FlowRange {
                    start: range.start,
                    end: split,
                    marker_floor: index + 1,
                };
                let right = FlowRange {
                    start: split,
                    end,
                    marker_floor: index + 1,
                };
                let known = if matches!(kind, ScopeKind::Route) {
                    actors.selected_route(range.start, split, end, 0)
                } else {
                    None
                };
                let selected = if matches!(kind, ScopeKind::Roll)
                    || matches!(kind, ScopeKind::Route)
                        && range.start <= self.earlier
                        && self.earlier < split
                {
                    Some(left)
                } else if matches!(kind, ScopeKind::Route)
                    && split <= self.earlier
                    && self.earlier < end
                {
                    Some(right)
                } else {
                    match known {
                        Some(SourceArm::Left) => Some(left),
                        Some(SourceArm::Right) => Some(right),
                        None => None,
                    }
                };
                let (after, fenced) = if let Some(selected) = selected {
                    match self.fifo_fence(actors, selected, entry, seen) {
                        Ok(value) => value,
                        Err(choice) => return Err(choice),
                    }
                } else {
                    let (a, af) = match self.fifo_fence(actors, left, entry, seen) {
                        Ok(value) => value,
                        Err(choice) => return Err(choice),
                    };
                    let (b, bf) = match self.fifo_fence(actors, right, entry, seen) {
                        Ok(value) => value,
                        Err(choice) => return Err(choice),
                    };
                    if matches!(kind, ScopeKind::Route) {
                        (a && b, af && bf)
                    } else {
                        (a || b, af || bf)
                    }
                };
                if fenced {
                    return Ok((after, true));
                }
                seen = after;
                range.start = end;
            } else {
                let atom = self.events.atom_at(range.start);
                if range.start == self.earlier {
                    seen = true;
                } else if seen
                    && atom.from == entry.from
                    && atom.to == entry.to
                    && atom.lane == entry.lane
                {
                    return Ok((true, true));
                }
                range.start += 1;
            }
        }
        Ok((seen, false))
    }

    const fn actor_precedes(
        &self,
        actors: &ActorMap<'_, E, D>,
        mut range: FlowRange,
        target: usize,
        actor: u8,
        iteration: usize,
    ) -> Result<bool, NeedChoice> {
        let prefix_end = if range.end < target {
            range.end
        } else {
            target
        };
        // These immutable sets contain every possible sender and receiver.
        // An absent actor needs no ownership-choice expansion or leaf scan.
        if range.start >= prefix_end
            || !self
                .scope_offsets
                .participants_in(range.start, prefix_end)
                .contains(actor)
        {
            return Ok(false);
        }
        let mut membership = CausalRoles::empty();
        membership.insert(actor);
        while range.start < range.end && range.start < target {
            if let Some((index, kind, split, end)) = self.scope_offsets.scope_at(&mut range) {
                let left = FlowRange {
                    start: range.start,
                    end: split,
                    marker_floor: index + 1,
                };
                let right = FlowRange {
                    start: split,
                    end,
                    marker_floor: index + 1,
                };
                let known = if matches!(kind, ScopeKind::Route) {
                    actors.selected_route(range.start, split, end, iteration)
                } else {
                    None
                };
                let selected =
                    if matches!(kind, ScopeKind::Roll) || range.start <= target && target < split {
                        Some(left)
                    } else if split <= target && target < end {
                        Some(right)
                    } else {
                        match known {
                            Some(SourceArm::Left) => Some(left),
                            Some(SourceArm::Right) => Some(right),
                            None => None,
                        }
                    };
                let present = if let Some(selected) = selected {
                    match self.actor_precedes(actors, selected, target, actor, iteration) {
                        Ok(value) => value,
                        Err(choice) => return Err(choice),
                    }
                } else {
                    let a = match self.actor_precedes(actors, left, target, actor, iteration) {
                        Ok(value) => value,
                        Err(choice) => return Err(choice),
                    };
                    let b = match self.actor_precedes(actors, right, target, actor, iteration) {
                        Ok(value) => value,
                        Err(choice) => return Err(choice),
                    };
                    if matches!(kind, ScopeKind::Route) {
                        a && b
                    } else {
                        a || b
                    }
                };
                if present {
                    return Ok(true);
                }
                range.start = end;
            } else {
                let atom = self.events.atom_at(range.start);
                let receiver = match actors.receiver(range.start, atom, iteration) {
                    Ok(value) => value,
                    Err(choice) => return Err(choice),
                };
                if receiver == actor {
                    return Ok(true);
                }
                match actors.sender_known(range.start, atom, iteration, &membership) {
                    Ok(true) => return Ok(true),
                    Ok(false) => {}
                    Err(choice) => return Err(choice),
                }
                range.start += 1;
            }
        }
        Ok(false)
    }

    const fn check(&self, paths: Paths<'_>, target: Option<usize>) -> bool {
        self.check_from(paths, target, self.range().start)
    }

    const fn check_from(&self, paths: Paths<'_>, target: Option<usize>, start: usize) -> bool {
        self.conflict_from(paths, target, start).is_none()
    }

    const fn conflict(&self, paths: Paths<'_>, target: Option<usize>) -> Option<usize> {
        self.conflict_from(paths, target, self.range().start)
    }

    const fn conflict_from(
        &self,
        paths: Paths<'_>,
        target: Option<usize>,
        start: usize,
    ) -> Option<usize> {
        let actors = match self.actors {
            Some(image) => ActorMap::Owned {
                image,
                events: image.event_actors(),
                paths,
                repeated: self.repeated,
            },
            None => ActorMap::Logical,
        };
        let choice = match self.run(&actors, target, start) {
            Ok(result) => return result,
            Err(choice) => choice,
        };
        let context = paths.get(choice.visit);
        let left = context.select(SourceSelection {
            scope: choice.scope,
            arm: SourceArm::Left,
        });
        let right = context.select(SourceSelection {
            scope: choice.scope,
            arm: SourceArm::Right,
        });
        let (left, right) = match choice.visit {
            Visit::Shared => (
                Paths {
                    shared: &left,
                    ..paths
                },
                Paths {
                    shared: &right,
                    ..paths
                },
            ),
            Visit::Current => (
                Paths {
                    current: &left,
                    ..paths
                },
                Paths {
                    current: &right,
                    ..paths
                },
            ),
            Visit::Next => (
                Paths {
                    next: &left,
                    ..paths
                },
                Paths {
                    next: &right,
                    ..paths
                },
            ),
        };
        // A current/shared choice excluding the earlier receive has no
        // obligation arising from it. Preserve this invariant when extending
        // the path instead of rescanning every source marker on each restart.
        // Next-visit choices stay independent of the earlier current visit.
        if !matches!(choice.visit, Visit::Next)
            && choice.scope.start as usize <= self.earlier
            && self.earlier < choice.scope.stop as usize
        {
            if self.earlier < choice.scope.split as usize {
                self.conflict_from(left, target, start)
            } else {
                self.conflict_from(right, target, start)
            }
        } else {
            match (
                self.conflict_from(left, target, start),
                self.conflict_from(right, target, start),
            ) {
                (Some(left), Some(right)) => Some(if left < right { left } else { right }),
                (Some(event), None) | (None, Some(event)) => Some(event),
                (None, None) => None,
            }
        }
    }
}

/// The ordering obligation for one possible shared receive direction. Lane
/// numbers do not enter this query: allocation uses it before choosing them.
pub(crate) const fn receive_pair_is_causal<const E: usize, const D: usize>(
    events: &EffList<E>,
    scope_offsets: &FlowIndex<E>,
    actors: Option<&SourceActorImage<E, D>>,
    earlier: usize,
    later: usize,
    closures: Option<ReceiveOrder<'_>>,
) -> bool {
    if earlier >= events.len() || later >= events.len() {
        return false;
    }
    let before = events.atom_at(earlier);
    let after = events.atom_at(later);
    if before.from == before.to
        || after.from == after.to
        || before.to != after.to
        || before.from == after.from
    {
        return true;
    }
    if scope_offsets.reentry_can_cross(earlier, later) {
        return false;
    }
    let empty = SourceContext::new(&[]);
    let paths = Paths {
        shared: &empty,
        current: &empty,
        next: &empty,
    };
    if earlier < later {
        let causal = match closures {
            Some(order) => order.contains(earlier, later),
            None => match scope_offsets.separating_scope(earlier, later) {
                Some(ScopeKind::Parallel) => false,
                Some(ScopeKind::Route) => true,
                _ => (Obligation {
                    events,
                    scope_offsets,
                    actors,
                    earlier,
                    end: later + 1,
                    repeated: None,
                    passive_entry: None,
                })
                .check(paths, Some(later)),
            },
        };
        if !causal {
            return false;
        }
    }
    let mut index = scope_offsets.enclosing_roll(earlier);
    while index != u16::MAX {
        let scope = scope_offsets.scopes[index as usize];
        let start = scope.start as usize;
        let end = scope.end as usize;
        if start <= later && later < end {
            let obligation = Obligation {
                events,
                scope_offsets,
                actors,
                earlier,
                end: later + 1,
                repeated: Some(RollBodyRange::new(start, end)),
                passive_entry: None,
            };
            if obligation
                .conflict_with_order(Some(later), closures)
                .is_some()
            {
                return false;
            }
        }
        index = scope_offsets.parent_roll(index);
    }
    true
}

/// Check source receive order and return its first rejected receive pair.
/// A new lane cannot distinguish visits of a passive, repeated choice. Its
/// alternative incoming heads still need a causal handoff between senders.
pub(crate) const fn passive_choice_conflict<const E: usize, const D: usize>(
    events: &EffList<E>,
    actors: Option<&SourceActorImage<E, D>>,
    indexed: Option<&FlowIndex<E>>,
) -> Option<(usize, usize, Option<u16>)> {
    let markers = events.scope_markers();
    let mut has_repeat = false;
    let mut has_choice = false;
    let mut marker = 0;
    while marker < markers.len() {
        let scope = markers.at(marker);
        if scope.event.is_primary_enter() {
            has_repeat |= matches!(scope.scope_id.kind(), Some(ScopeKind::Roll));
            has_choice |= matches!(scope.scope_id.kind(), Some(ScopeKind::Route));
        }
        marker += 1;
    }
    if !has_repeat || !has_choice {
        return None;
    }
    let mut first_sender = [u16::MAX; 256];
    let mut mixed = CausalRoles::empty();
    let mut event = 0;
    while event < events.len() {
        let atom = events.atom_at(event);
        if atom.from != atom.to {
            let first = first_sender[atom.to as usize];
            if first == u16::MAX {
                first_sender[atom.to as usize] = atom.from as u16;
            } else if first != atom.from as u16 {
                mixed.insert(atom.to);
            }
        }
        event += 1;
    }
    if mixed.is_empty() {
        return None;
    }
    let built;
    let scopes = match indexed {
        Some(indexed) => indexed,
        None => {
            built = FlowIndex::new(events, actors);
            &built
        }
    };
    let context = SourceContext::new(&[]);
    let paths = Paths {
        shared: &context,
        current: &context,
        next: &context,
    };
    let mut route = 0;
    while route < scopes.marker_len {
        let scope = scopes.scopes[route];
        if matches!(scope.kind, ScopeKind::Route) && scope.roll != u16::MAX {
            let roll = scopes.scopes[scope.roll as usize];
            let body = RollBodyRange::new(roll.start as usize, roll.end as usize);
            let actor_map = match actors {
                Some(image) => ActorMap::Owned {
                    image,
                    events: image.event_actors(),
                    paths,
                    repeated: Some(body),
                },
                None => ActorMap::Logical,
            };
            // A guaranteed earlier local action makes this receive an active
            // continuation. Establish that once per row, before pairing heads.
            // Empty shared/current/next contexts make this a universal fact;
            // an unresolved holder remains in the exact path-specific check.
            let mut active = [false; E];
            let mut event = scope.start as usize;
            while event < scope.end as usize {
                let atom = events.atom_at(event);
                if atom.from != atom.to
                    && mixed.contains(atom.to)
                    && let Ok(actor) = actor_map.receiver(event, atom, 0)
                {
                    let mut local = CausalRoles::empty();
                    local.insert(actor);
                    let prior = Obligation {
                        events,
                        scope_offsets: scopes,
                        actors,
                        earlier: event,
                        end: event + 1,
                        repeated: Some(body),
                        passive_entry: Some(route),
                    };
                    let range = if event < scope.split as usize {
                        FlowRange {
                            start: scope.start as usize,
                            end: scope.split as usize,
                            marker_floor: route + 1,
                        }
                    } else {
                        FlowRange {
                            start: scope.split as usize,
                            end: scope.end as usize,
                            marker_floor: route + 1,
                        }
                    };
                    active[event] =
                        matches!(actor_map.sender_known(event, atom, 0, &local), Ok(true))
                            || matches!(
                                prior.actor_precedes(&actor_map, range, event, actor, 0),
                                Ok(true)
                            );
                }
                event += 1;
            }
            let mut earlier = scope.start as usize;
            while earlier < scope.end as usize {
                let before = events.atom_at(earlier);
                if !active[earlier] && before.from != before.to && mixed.contains(before.to) {
                    let mut later = scopes.first_receive_to(before.to);
                    while later != u16::MAX && later < scope.end {
                        let after = events.atom_at(later as usize);
                        if later >= scope.start
                            && !active[later as usize]
                            && (earlier < scope.split as usize) != (later < scope.split)
                            && before.from != after.from
                        {
                            let obligation = Obligation {
                                events,
                                scope_offsets: scopes,
                                actors,
                                earlier,
                                end: later as usize + 1,
                                repeated: Some(body),
                                passive_entry: Some(route),
                            };
                            if obligation.conflict(paths, Some(later as usize)).is_some() {
                                return Some((
                                    earlier,
                                    later as usize,
                                    Some(markers.at(route).scope_id.local_ordinal()),
                                ));
                            }
                        }
                        later = scopes.next_receive_to(later);
                    }
                }
                earlier += 1;
            }
        }
        route += 1;
    }
    None
}

pub(crate) const fn owned_receive_lane_conflict<const E: usize, const D: usize>(
    events: &EffList<E>,
    actors: &SourceActorImage<E, D>,
) -> Option<(usize, usize, Option<u16>)> {
    let scope_offsets = FlowIndex::new(events, Some(actors));
    receive_lane_conflict_in(
        events,
        &scope_offsets,
        Some(actors),
        ReceiveScan::From(None),
        None,
    )
}

/// Source order is the whole-source region, followed by each roll body.
/// A before/from pair partitions the seed queries, not their target receives.
#[derive(Clone, Copy)]
pub(crate) enum ReceiveScan {
    From(Option<(usize, Option<u16>)>),
    Before((usize, Option<u16>)),
}

pub(crate) const fn receive_lane_conflict_in<const E: usize, const D: usize>(
    events: &EffList<E>,
    scope_offsets: &FlowIndex<E>,
    actors: Option<&SourceActorImage<E, D>>,
    scan: ReceiveScan,
    closures: Option<ReceiveOrder<'_>>,
) -> Option<(usize, usize, Option<u16>)> {
    let mut direction_storage = [Direction::EMPTY; E];
    let mut direction_storage_homogeneous = [false; E];
    let directions = ReceiveDirectionIndex::new(
        events,
        &mut direction_storage,
        &mut direction_storage_homogeneous,
    );
    receive_lane_conflict_indexed(events, scope_offsets, actors, scan, closures, &directions)
}

pub(crate) const fn receive_lane_conflict_indexed<const E: usize, const D: usize>(
    events: &EffList<E>,
    scope_offsets: &FlowIndex<E>,
    actors: Option<&SourceActorImage<E, D>>,
    scan: ReceiveScan,
    closures: Option<ReceiveOrder<'_>>,
    directions: &ReceiveDirectionIndex<'_>,
) -> Option<(usize, usize, Option<u16>)> {
    let mut resume = match scan {
        ReceiveScan::From(resume) => resume,
        ReceiveScan::Before(_) => None,
    };
    let mut region = 0;
    let markers = events.scope_markers();
    let homogeneous = directions.homogeneous_rows();
    while region <= markers.len() {
        let (start, stop, repeated, scope) = if region == 0 {
            (0, events.len(), None, None)
        } else {
            let marker = markers.at(region - 1);
            if !marker.event.is_primary_enter()
                || !matches!(marker.scope_id.kind(), Some(ScopeKind::Roll))
            {
                region += 1;
                continue;
            }
            let Some((start, stop)) = roll_body_range_from_enter(markers, region - 1) else {
                crate::invariant();
            };
            if !RollBodyRange::new(start, stop).is_valid_for(events) {
                crate::invariant();
            }
            (
                start,
                stop,
                Some(RollBodyRange::new(start, stop)),
                Some(marker.scope_id.local_ordinal()),
            )
        };
        let stop_before = match (scan, scope) {
            (ReceiveScan::Before((event, None)), None) => Some(event),
            (ReceiveScan::Before((event, Some(cut))), Some(scope)) if cut == scope => Some(event),
            _ => None,
        };
        if let Some(event) = stop_before
            && (event < start || event >= stop)
        {
            crate::invariant();
        }
        let mut earlier = match resume {
            Some((earlier, resume_scope)) => {
                let same_scope = match (scope, resume_scope) {
                    (Some(a), Some(b)) => a == b,
                    (None, None) => true,
                    _ => false,
                };
                if !same_scope {
                    region += 1;
                    continue;
                }
                if earlier < start || earlier >= stop {
                    crate::invariant();
                }
                resume = None;
                earlier
            }
            None => start,
        };
        while earlier < stop {
            if let Some(event) = stop_before
                && earlier == event
            {
                return None;
            }
            if homogeneous[earlier] {
                earlier += 1;
                continue;
            }
            let atom = events.atom_at(earlier);
            let range = FlowRange {
                start: if repeated.is_some() {
                    start
                } else {
                    earlier + 1
                },
                end: stop,
                marker_floor: 0,
            };
            if atom.from != atom.to
                && let Some(end) = directions.last_change(atom, (range.start, stop))
            {
                let reentry = if repeated.is_none() {
                    scope_offsets.first_reentry_conflict(events, earlier, end)
                } else {
                    None
                };
                let ordinary = if repeated.is_none() && closures.is_some() {
                    let order = match closures {
                        Some(order) => order,
                        None => crate::invariant(),
                    };
                    let mut later = scope_offsets.next_receive_to(earlier as u16);
                    let mut rejected = None;
                    while later != u16::MAX && (later as usize) < end {
                        let candidate = later as usize;
                        if (FlowGoal::ReceiveLane(atom, end))
                            .sender_change(candidate, events.atom_at(candidate))
                            && !order.contains(earlier, candidate)
                        {
                            rejected = Some(candidate);
                            break;
                        }
                        later = scope_offsets.next_receive_to(later);
                    }
                    rejected
                } else {
                    (Obligation {
                        events,
                        scope_offsets,
                        actors,
                        earlier,
                        end,
                        repeated,
                        passive_entry: None,
                    })
                    .conflict_with_order(None, closures)
                };
                let later = match (reentry, ordinary) {
                    (Some(a), Some(b)) => Some(if a < b { a } else { b }),
                    (a, None) | (None, a) => a,
                };
                let Some(later) = later else {
                    earlier += 1;
                    continue;
                };
                if later < range.start
                    || later >= end
                    || !(FlowGoal::ReceiveLane(atom, later + 1))
                        .sender_change(later, events.atom_at(later))
                {
                    crate::invariant();
                }
                return Some((earlier, later, scope));
            }
            earlier += 1;
        }
        region += 1;
    }
    if resume.is_some() || matches!(scan, ReceiveScan::Before(_)) {
        crate::invariant();
    }
    None
}
