//! Receive-order obligations use logical peers and lanes. Causal relays use
//! the physical actor holding each endpoint at that source event. Both come
//! from the same checked source; possible-holder sets are never authority.
use super::{
    CausalFlow, CausalRoles, EffList, FlowGoal, FlowRange, RollBodyRange, ScopeKind,
    roll_body_range_from_enter, sender_change_end,
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

#[derive(Clone, Copy)]
pub(super) enum ActorMap<'a, const E: usize, const D: usize> {
    Logical,
    Owned {
        image: &'a SourceActorImage<E, D>,
        paths: Paths<'a>,
        repeated: Option<RollBodyRange>,
    },
}

impl<const E: usize, const D: usize> ActorMap<'_, E, D> {
    const fn visit(self, scope: SourceScope, iteration: usize) -> Visit {
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

    const fn selection(self, scope: SourceScope, iteration: usize) -> Option<SourceArm> {
        match self {
            Self::Logical => None,
            Self::Owned { paths, .. } => paths.get(self.visit(scope, iteration)).get(scope),
        }
    }

    pub(super) const fn selected_route(
        self,
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

    const fn reference(self, event: usize, side: usize) -> u16 {
        let Self::Owned { image, .. } = self else {
            crate::invariant()
        };
        match image.event_references(event) {
            Some(references) => references[side],
            None => crate::invariant(),
        }
    }

    const fn decision(self, reference: u16) -> (SourceScope, [u16; 2]) {
        let Self::Owned { image, .. } = self else {
            crate::invariant()
        };
        match image.decision(reference as usize - 256) {
            Some(value) => value,
            None => crate::invariant(),
        }
    }

    pub(super) const fn receiver(
        self,
        event: usize,
        atom: EffAtom,
        iteration: usize,
    ) -> Result<u8, NeedChoice> {
        if let Self::Logical = self {
            return Ok(atom.to);
        }
        let mut reference = self.reference(event, 1);
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
        self,
        reference: u16,
        iteration: usize,
        facts: CausalRoles,
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
        self,
        event: usize,
        atom: EffAtom,
        iteration: usize,
        facts: CausalRoles,
    ) -> Result<bool, NeedChoice> {
        match self {
            Self::Logical => Ok(facts.contains(atom.from)),
            Self::Owned { .. } => self.reference_known(self.reference(event, 0), iteration, facts),
        }
    }
}

struct Obligation<'a, const E: usize, const D: usize> {
    events: &'a EffList<E>,
    actors: &'a SourceActorImage<E, D>,
    earlier: usize,
    end: usize,
    repeated: Option<RollBodyRange>,
}

impl<const E: usize, const D: usize> Obligation<'_, E, D> {
    const fn run(&self, actors: ActorMap<'_, E, D>) -> Result<bool, NeedChoice> {
        let earlier = self.events.atom_at(self.earlier);
        let body = match self.repeated {
            Some(body) => body,
            None => RollBodyRange::new(0, self.events.len()),
        };
        let range = FlowRange {
            start: body.start,
            end: body.end,
            marker_floor: 0,
        };
        let flow = CausalFlow {
            eff_list: self.events,
            earlier: self.earlier - body.start,
            goal: match self.repeated {
                Some(_) => FlowGoal::Closure,
                None => FlowGoal::ReceiveLane(earlier, self.end),
            },
            body_start: body.start,
            iteration_start: 0,
        };
        let facts = match flow.advance_as(range, CausalRoles::empty(), actors) {
            Ok(Some(value)) => value,
            Ok(None) => return Ok(false),
            Err(choice) => return Err(choice),
        };
        if self.repeated.is_none() {
            return Ok(true);
        }
        let next = CausalFlow {
            goal: FlowGoal::ReceiveLane(earlier, body.len() + self.end - body.start),
            iteration_start: body.len(),
            ..flow
        };
        match next.advance_as(range, facts, actors) {
            Ok(result) => Ok(result.is_some()),
            Err(choice) => Err(choice),
        }
    }

    const fn check(&self, paths: Paths<'_>) -> bool {
        let choice = match self.run(ActorMap::Owned {
            image: self.actors,
            paths,
            repeated: self.repeated,
        }) {
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
                self.check(left)
            } else {
                self.check(right)
            }
        } else {
            self.check(left) && self.check(right)
        }
    }
}

pub(crate) const fn validate_owned_receive_lane_causality<const E: usize, const D: usize>(
    events: &EffList<E>,
    actors: &SourceActorImage<E, D>,
) -> bool {
    let context = SourceContext::new(&[]);
    let paths = Paths {
        shared: &context,
        current: &context,
        next: &context,
    };
    let range = FlowRange {
        start: 0,
        end: events.len(),
        marker_floor: 0,
    };
    let mut earlier = 0;
    while earlier < events.len() {
        let atom = events.atom_at(earlier);
        if atom.from != atom.to
            && let Some(end) = sender_change_end(
                events,
                FlowRange {
                    start: earlier + 1,
                    ..range
                },
                atom,
            )
            && !(Obligation {
                events,
                actors,
                earlier,
                end,
                repeated: None,
            })
            .check(paths)
        {
            return false;
        }
        earlier += 1;
    }
    let markers = events.scope_markers();
    let mut index = 0;
    while index < markers.len() {
        let marker = markers.at(index);
        if marker.event.is_primary_enter()
            && matches!(marker.scope_id.kind(), Some(ScopeKind::Roll))
        {
            let Some((start, end)) = roll_body_range_from_enter(markers, index) else {
                return false;
            };
            let body = RollBodyRange::new(start, end);
            if !body.is_valid_for(events) {
                return false;
            }
            let mut earlier = start;
            while earlier < end {
                let atom = events.atom_at(earlier);
                if atom.from != atom.to
                    && let Some(end) = sender_change_end(
                        events,
                        FlowRange {
                            start,
                            end,
                            marker_floor: 0,
                        },
                        atom,
                    )
                    && !(Obligation {
                        events,
                        actors,
                        earlier,
                        end,
                        repeated: Some(body),
                    })
                    .check(paths)
                {
                    return false;
                }
                earlier += 1;
            }
        }
        index += 1;
    }
    true
}

/// Locate the rejected receive pair without changing projection admission.
pub(crate) const fn owned_receive_lane_conflict<const E: usize, const D: usize>(
    events: &EffList<E>,
    actors: &SourceActorImage<E, D>,
) -> Option<(usize, usize, Option<u16>)> {
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
                return None;
            };
            (
                start,
                stop,
                Some(RollBodyRange::new(start, stop)),
                Some(marker.scope_id.local_ordinal()),
            )
        };
        let mut earlier = start;
        while earlier < stop {
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
                && let Some(end) = sender_change_end(events, range, atom)
                && !(Obligation {
                    events,
                    actors,
                    earlier,
                    end,
                    repeated,
                })
                .check(paths)
            {
                let mut later = range.start;
                while later < end {
                    if (FlowGoal::ReceiveLane(atom, later + 1)).sender_change(events.atom_at(later))
                        && !(Obligation {
                            events,
                            actors,
                            earlier,
                            end: later + 1,
                            repeated,
                        })
                        .check(paths)
                    {
                        return Some((earlier, later, scope));
                    }
                    later += 1;
                }
            }
            earlier += 1;
        }
        region += 1;
    }
    None
}
