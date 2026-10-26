//! Receive-order obligations use logical peers and lanes. Causal relays use
//! the physical actor holding each endpoint at that source event. Both come
//! from the same checked source; possible-holder sets are never authority.
use super::{
    CausalFlow, CausalRoles, EffList, FlowGoal, FlowRange, ReceiveDirectionIndex, RollBodyRange,
    ScopeKind, ScopeOffsets, roll_body_range_from_enter,
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
    scope_offsets: &'a ScopeOffsets<E>,
    actors: Option<&'a SourceActorImage<E, D>>,
    earlier: usize,
    end: usize,
    repeated: Option<RollBodyRange>,
}

impl<const E: usize, const D: usize> Obligation<'_, E, D> {
    const fn range(&self) -> RollBodyRange {
        match self.repeated {
            Some(body) => body,
            None => RollBodyRange::new(0, self.events.len()),
        }
    }

    const fn run(
        &self,
        actors: &ActorMap<'_, E, D>,
        target: Option<usize>,
        start: usize,
    ) -> Result<Option<usize>, NeedChoice> {
        let earlier = self.events.atom_at(self.earlier);
        let body = self.range();
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
        let next = CausalFlow {
            goal: match target {
                Some(event) => FlowGoal::ReceiveEvent(earlier, body.len() + event - body.start),
                None => FlowGoal::ReceiveLane(earlier, body.len() + self.end - body.start),
            },
            iteration_start: body.len(),
            ..flow
        };
        match next.advance_as(range, facts, actors) {
            Ok(Ok(_)) => Ok(None),
            Ok(Err(event)) => Ok(Some(body.start + event - body.len())),
            Err(choice) => Err(choice),
        }
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
    scope_offsets: &ScopeOffsets<E>,
    actors: Option<&SourceActorImage<E, D>>,
    earlier: usize,
    later: usize,
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
    let empty = SourceContext::new(&[]);
    let paths = Paths {
        shared: &empty,
        current: &empty,
        next: &empty,
    };
    if earlier < later
        && !(Obligation {
            events,
            scope_offsets,
            actors,
            earlier,
            end: later + 1,
            repeated: None,
        })
        .check(paths, Some(later))
    {
        return false;
    }
    let markers = events.scope_markers();
    let mut index = 0;
    while index < markers.len() {
        let marker = markers.at(index);
        // Markers are ordered by source offset. A later start cannot contain
        // both receives, including when their order crosses a roll boundary.
        if marker.offset() > earlier || marker.offset() > later {
            break;
        }
        if !marker.event.is_primary_enter()
            || !matches!(marker.scope_id.kind(), Some(ScopeKind::Roll))
        {
            index += 1;
            continue;
        }
        let start = marker.offset();
        let end = marker.segment_end();
        if end <= start {
            return false;
        }
        if earlier < end
            && later < end
            && !(Obligation {
                events,
                scope_offsets,
                actors,
                earlier,
                end: later + 1,
                repeated: Some(RollBodyRange::new(start, end)),
            })
            .check(paths, Some(later))
        {
            return false;
        }
        index += 1;
    }
    true
}

/// Check source receive order and return its first rejected receive pair.
pub(crate) const fn owned_receive_lane_conflict<const E: usize, const D: usize>(
    events: &EffList<E>,
    actors: &SourceActorImage<E, D>,
) -> Option<(usize, usize, Option<u16>)> {
    let scope_offsets = ScopeOffsets::new(events);
    owned_receive_lane_conflict_in(events, &scope_offsets, actors, ReceiveScan::From(None))
}

/// Source order is the whole-source region, followed by each roll body.
/// A before/from pair partitions the seed queries, not their target receives.
#[derive(Clone, Copy)]
pub(crate) enum ReceiveScan {
    From(Option<(usize, Option<u16>)>),
    Before((usize, Option<u16>)),
}

pub(crate) const fn owned_receive_lane_conflict_in<const E: usize, const D: usize>(
    events: &EffList<E>,
    scope_offsets: &ScopeOffsets<E>,
    actors: &SourceActorImage<E, D>,
    scan: ReceiveScan,
) -> Option<(usize, usize, Option<u16>)> {
    let mut resume = match scan {
        ReceiveScan::From(resume) => resume,
        ReceiveScan::Before(_) => None,
    };
    let context = SourceContext::new(&[]);
    let paths = Paths {
        shared: &context,
        current: &context,
        next: &context,
    };
    let mut region = 0;
    let markers = events.scope_markers();
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
        let directions = ReceiveDirectionIndex::new(events, start, stop);
        while earlier < stop {
            if let Some(event) = stop_before
                && earlier == event
            {
                return None;
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
                && let Some(end) = directions.last_change(atom, range.start)
                && let Some(later) = (Obligation {
                    events,
                    scope_offsets,
                    actors: Some(actors),
                    earlier,
                    end,
                    repeated,
                })
                .conflict(paths, None)
            {
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
