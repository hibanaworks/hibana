//! Knowledge of choice for the actors that hold the projected endpoints.

use super::super::{Arm, Context, Scope, Selection, Source};
use super::{ActorImage, ActorRef};

#[cfg(kani)]
#[path = "knowledge/kani.rs"]
mod kani;

#[cfg(all(test, hibana_repo_tests))]
mod tests {
    use super::*;

    #[test]
    fn scoped_participant_filter_preserves_every_actor_and_correlated_path() {
        let prior = Scope {
            start: 0,
            split: 1,
            stop: 2,
        };
        let other = Scope {
            start: 2,
            split: 3,
            stop: 4,
        };
        let image = ActorImage::<6, 3>::causal_fixture(
            &[
                [255, 256],
                [255, 257],
                [258, 1],
                [2, 258],
                [254, 0],
                [0, 255],
            ],
            &[(prior, [0, 1]), (prior, [1, 0]), (other, [256, 257])],
        );
        let checker = Checker { image: &image };
        let mut comparisons = 0;
        for first in [None, Some(Arm::Left), Some(Arm::Right)] {
            for second in [None, Some(Arm::Left), Some(Arm::Right)] {
                let context = Context::new(&[]);
                let first_path;
                let context = if let Some(arm) = first {
                    first_path = context.select(Selection { scope: prior, arm });
                    &first_path
                } else {
                    &context
                };
                let second_path;
                let context = if let Some(arm) = second {
                    second_path = context.select(Selection { scope: other, arm });
                    &second_path
                } else {
                    context
                };
                for start in 0..4 {
                    for split in start + 1..6 {
                        for stop in split + 1..=6 {
                            let scope = Scope { start, split, stop };
                            let candidates = checker.participants(scope);
                            let controller = image.events[start as usize].sender;
                            for role in 0u16..=255 {
                                let full = checker
                                    .matching_presence(scope, role as u8, controller, context);
                                let filtered =
                                    candidates[role as usize / 8] & (1 << (role % 8)) == 0 || full;
                                assert_eq!(
                                    filtered, full,
                                    "scope={start}/{split}/{stop} role={role}"
                                );
                                comparisons += 1;
                            }
                        }
                    }
                }
            }
        }
        assert_eq!(comparisons, 78_336);
        let candidates = checker.participants(prior);
        assert!(candidates[0] & 1 != 0);
        assert!(!checker.matching_presence(prior, 0, ActorRef(255), &Context::new(&[])));
    }
}

pub(super) const fn check<const E: usize, const D: usize>(
    source: &Source<'_>,
    image: &ActorImage<E, D>,
) -> bool {
    conflict(source, image).is_none()
}

pub(super) const fn conflict<const E: usize, const D: usize>(
    source: &Source<'_>,
    image: &ActorImage<E, D>,
) -> Option<(Scope, Option<u8>, Option<usize>)> {
    if source.event_count() != image.event_len {
        panic!("choice-knowledge source and actor image disagree");
    }
    Checker { image }.nested_conflict(source, 0, &Context::new(&[]))
}

struct Checker<'a, const E: usize, const D: usize> {
    image: &'a ActorImage<E, D>,
}

enum Presence {
    Known(bool),
    Choice(Scope),
}

const fn route_scope(start: usize, left: &Source<'_>, right: &Source<'_>) -> Scope {
    let split = start + left.event_count();
    let stop = split + right.event_count();
    if stop > u16::MAX as usize {
        panic!("choice source exceeds event domain");
    }
    Scope {
        start: start as u16,
        split: split as u16,
        stop: stop as u16,
    }
}

impl<const E: usize, const D: usize> Checker<'_, E, D> {
    /// Roles absent from every possible endpoint holder in this choice have
    /// equal, false participation in both arms. The union only excludes these
    /// absent roles; remaining roles still use the correlated path check.
    const fn participants(&self, scope: Scope) -> [u8; 32] {
        let mut participants = [0; 32];
        let mut event = scope.start as usize;
        while event < scope.stop as usize {
            let pair = self.image.events[event];
            if pair.sender.0 < 256 && pair.receiver.0 < 256 {
                participants[pair.sender.0 as usize / 8] |= 1 << (pair.sender.0 % 8);
                participants[pair.receiver.0 as usize / 8] |= 1 << (pair.receiver.0 % 8);
            } else {
                let candidates = self.image.possible_actors(event);
                let mut word = 0;
                while word < participants.len() {
                    participants[word] |= candidates[word];
                    word += 1;
                }
            }
            event += 1;
        }
        participants
    }

    const fn selected(&self, mut reference: ActorRef, path: &Context<'_>) -> ActorRef {
        while reference.0 >= 256 {
            let index = reference.0 as usize - 256;
            if index >= self.image.choice_len {
                panic!("choice reference outside image");
            }
            let choice = self.image.choices[index];
            match path.get(choice.scope) {
                Some(Arm::Left) => reference = choice.left,
                Some(Arm::Right) => reference = choice.right,
                None => break,
            }
        }
        reference
    }

    /// Universal equality under the borrowed canonical path. Splitting a
    /// remaining decision constrains both references, preserving correlations.
    const fn equivalent(&self, left: ActorRef, right: ActorRef, path: &Context<'_>) -> bool {
        let left = self.selected(left, path);
        let right = self.selected(right, path);
        if left.0 == right.0 {
            return true;
        }
        let (choice, split_left) = if left.0 >= 256 {
            (self.image.choices[left.0 as usize - 256], true)
        } else if right.0 >= 256 {
            (self.image.choices[right.0 as usize - 256], false)
        } else {
            return false;
        };
        let left_path = path.select(Selection {
            scope: choice.scope,
            arm: Arm::Left,
        });
        let right_path = path.select(Selection {
            scope: choice.scope,
            arm: Arm::Right,
        });
        if split_left {
            self.equivalent(choice.left, right, &left_path)
                && self.equivalent(choice.right, right, &right_path)
        } else {
            self.equivalent(left, choice.left, &left_path)
                && self.equivalent(left, choice.right, &right_path)
        }
    }

    /// A guaranteed incoming message teaches the branch to its physical
    /// recipient. Seq and completed par combine knowledge. Every alternative
    /// of an inner route must supply it before its common continuation.
    const fn after(
        &self,
        source: &Source<'_>,
        start: usize,
        actor: ActorRef,
        path: &Context<'_>,
    ) -> bool {
        match source {
            Source::Send(message) => {
                !message.from.same(message.to)
                    && self.equivalent(self.image.events[start].receiver, actor, path)
            }
            Source::Seq(left, right, _) | Source::Par(left, right, _) => {
                self.after(left, start, actor, path)
                    || self.after(right, start + left.event_count(), actor, path)
            }
            Source::Route { left, right, .. } => {
                let scope = route_scope(start, left, right);
                let left_path = path.select(Selection {
                    scope,
                    arm: Arm::Left,
                });
                let right_path = path.select(Selection {
                    scope,
                    arm: Arm::Right,
                });
                self.after(left, start, actor, &left_path)
                    && self.after(right, scope.split as usize, actor, &right_path)
            }
            Source::Roll(body, _) => self.after(body, start, actor, path),
        }
    }

    const fn before(
        &self,
        source: &Source<'_>,
        start: usize,
        event: usize,
        actor: ActorRef,
        controller: ActorRef,
        path: &Context<'_>,
    ) -> bool {
        if self.equivalent(actor, controller, path) {
            return true;
        }
        match source {
            Source::Send(_) => false,
            Source::Seq(left, right, _) => {
                let split = start + left.event_count();
                if event < split {
                    self.before(left, start, event, actor, controller, path)
                } else {
                    self.after(left, start, actor, path)
                        || self.before(right, split, event, actor, controller, path)
                }
            }
            Source::Par(left, right, _) => {
                let split = start + left.event_count();
                // A sibling's receive cannot authorize an independently
                // enabled send before the parallel join.
                if event < split {
                    self.before(left, start, event, actor, controller, path)
                } else {
                    self.before(right, split, event, actor, controller, path)
                }
            }
            Source::Route { left, right, .. } => {
                let scope = route_scope(start, left, right);
                if event < scope.split as usize {
                    let selected = path.select(Selection {
                        scope,
                        arm: Arm::Left,
                    });
                    self.before(left, start, event, actor, controller, &selected)
                } else {
                    let selected = path.select(Selection {
                        scope,
                        arm: Arm::Right,
                    });
                    self.before(
                        right,
                        scope.split as usize,
                        event,
                        actor,
                        controller,
                        &selected,
                    )
                }
            }
            Source::Roll(body, _) => self.before(body, start, event, actor, controller, path),
        }
    }

    const fn arm(
        &self,
        source: &Source<'_>,
        start: usize,
        controller: ActorRef,
        path: &Context<'_>,
    ) -> bool {
        let stop = start + source.event_count();
        let mut event = start;
        while event < stop {
            let actor = self.image.events[event].sender;
            if !self.before(source, start, event, actor, controller, path) {
                return false;
            }
            event += 1;
        }
        true
    }

    const fn reference_presence(
        &self,
        reference: ActorRef,
        role: u8,
        path: &Context<'_>,
    ) -> Presence {
        let reference = self.selected(reference, path);
        if reference.0 < 256 {
            return Presence::Known(reference.0 == role as u16);
        }
        // Let the common participation query split this canonical decision
        // once for all references. Recursively expanding both children here
        // repeats that work and obscures their shared-choice correlation.
        let choice = self.image.choices[reference.0 as usize - 256];
        Presence::Choice(choice.scope)
    }

    const fn member_presence(
        &self,
        start: usize,
        stop: usize,
        role: u8,
        path: &Context<'_>,
    ) -> Presence {
        let mut unresolved = None;
        let mut event = start;
        while event < stop {
            let pair = self.image.events[event];
            let references = [pair.sender, pair.receiver];
            let mut side = 0;
            while side < 2 {
                match self.reference_presence(references[side], role, path) {
                    Presence::Known(true) => return Presence::Known(true),
                    Presence::Known(false) => {}
                    Presence::Choice(scope) => {
                        if unresolved.is_none() {
                            unresolved = Some(scope);
                        }
                    }
                }
                side += 1;
            }
            event += 1;
        }
        match unresolved {
            Some(scope) => Presence::Choice(scope),
            None => Presence::Known(false),
        }
    }

    /// Compare actual participation under every shared prior choice. A union
    /// of candidate actors would lose correlations and can accept an actor
    /// present only in one arm of an actual execution.
    const fn matching_presence(
        &self,
        scope: Scope,
        role: u8,
        controller: ActorRef,
        path: &Context<'_>,
    ) -> bool {
        let controller_presence = self.reference_presence(controller, role, path);
        if matches!(controller_presence, Presence::Known(true)) {
            return true;
        }
        let left = self.member_presence(scope.start as usize, scope.split as usize, role, path);
        let right = self.member_presence(scope.split as usize, scope.stop as usize, role, path);
        if let (Presence::Known(left), Presence::Known(right)) = (&left, &right) {
            if *left == *right {
                return true;
            }
            if matches!(controller_presence, Presence::Known(false)) {
                return false;
            }
        }
        let decision = match (controller_presence, left, right) {
            (Presence::Choice(scope), _, _)
            | (_, Presence::Choice(scope), _)
            | (_, _, Presence::Choice(scope)) => scope,
            _ => panic!("unclassified actor participation"),
        };
        let left_path = path.select(Selection {
            scope: decision,
            arm: Arm::Left,
        });
        let right_path = path.select(Selection {
            scope: decision,
            arm: Arm::Right,
        });
        self.matching_presence(scope, role, controller, &left_path)
            && self.matching_presence(scope, role, controller, &right_path)
    }

    const fn nested_conflict(
        &self,
        source: &Source<'_>,
        start: usize,
        path: &Context<'_>,
    ) -> Option<(Scope, Option<u8>, Option<usize>)> {
        match source {
            Source::Send(_) => None,
            Source::Seq(left, right, _) | Source::Par(left, right, _) => {
                if let Some(conflict) = self.nested_conflict(left, start, path) {
                    return Some(conflict);
                }
                self.nested_conflict(right, start + left.event_count(), path)
            }
            Source::Roll(body, _) => self.nested_conflict(body, start, path),
            Source::Route { left, right, .. } => {
                let scope = route_scope(start, left, right);
                let controller = self.image.events[start].sender;
                // Keep the current conservative merge rule: a noncontroller
                // actor must participate in both alternatives or neither.
                let participants = self.participants(scope);
                let mut role = 0usize;
                while role < 256 {
                    if participants[role / 8] & (1 << (role % 8)) != 0
                        && !self.matching_presence(scope, role as u8, controller, path)
                    {
                        return Some((scope, Some(role as u8), None));
                    }
                    role += 1;
                }
                let left_path = path.select(Selection {
                    scope,
                    arm: Arm::Left,
                });
                let right_path = path.select(Selection {
                    scope,
                    arm: Arm::Right,
                });
                let arms = [
                    (left, start, &left_path),
                    (right, scope.split as usize, &right_path),
                ];
                let mut arm = 0;
                while arm < arms.len() {
                    let (body, begin, selected) = arms[arm];
                    if !self.arm(body, begin, controller, selected) {
                        let mut event = begin;
                        while event < begin + body.event_count() {
                            let actor = self.image.events[event].sender;
                            if !self.before(body, begin, event, actor, controller, selected) {
                                let actor = self.selected(actor, selected);
                                let role = if actor.0 < 256 {
                                    Some(actor.0 as u8)
                                } else {
                                    None
                                };
                                return Some((scope, role, Some(event)));
                            }
                            event += 1;
                        }
                    }
                    arm += 1;
                }
                if let Some(conflict) = self.nested_conflict(left, start, &left_path) {
                    return Some(conflict);
                }
                self.nested_conflict(right, scope.split as usize, &right_path)
            }
        }
    }
}
