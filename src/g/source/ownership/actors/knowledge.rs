//! Construction-only knowledge of choice over the existing typed source.
//! Public owned projection stays closed until the complete acceptance boundary
//! and its independent verification are connected.
//! Ownership references denote actor functions of canonical source choices.
//! No current-holder table or runtime branch authority is introduced here.

use super::super::{Arm, Context, Scope, Selection, Source};
use super::{ActorImage, ActorRef};

#[cfg(kani)]
#[path = "knowledge/kani.rs"]
mod kani;

pub(super) const fn check<const E: usize, const D: usize>(
    source: &Source<'_>,
    image: &ActorImage<E, D>,
) -> bool {
    if source.event_count() != image.event_len {
        panic!("choice-knowledge source and actor image disagree");
    }
    Checker { image }.nested(source, 0, &Context::new(&[]))
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
            Source::Seq(left, right) | Source::Par(left, right) => {
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
            Source::Roll(body) => self.after(body, start, actor, path),
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
            Source::Seq(left, right) => {
                let split = start + left.event_count();
                if event < split {
                    self.before(left, start, event, actor, controller, path)
                } else {
                    self.after(left, start, actor, path)
                        || self.before(right, split, event, actor, controller, path)
                }
            }
            Source::Par(left, right) => {
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
            Source::Roll(body) => self.before(body, start, event, actor, controller, path),
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

    const fn nested(&self, source: &Source<'_>, start: usize, path: &Context<'_>) -> bool {
        match source {
            Source::Send(_) => true,
            Source::Seq(left, right) | Source::Par(left, right) => {
                self.nested(left, start, path)
                    && self.nested(right, start + left.event_count(), path)
            }
            Source::Roll(body) => self.nested(body, start, path),
            Source::Route { left, right, .. } => {
                let scope = route_scope(start, left, right);
                let controller = self.image.events[start].sender;
                // Keep the current conservative merge rule: a noncontroller
                // actor must participate in both alternatives or neither.
                let mut role = 0usize;
                while role < 256 {
                    if !self.matching_presence(scope, role as u8, controller, path) {
                        return false;
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
                self.arm(left, start, controller, &left_path)
                    && self.arm(right, scope.split as usize, controller, &right_path)
                    && self.nested(left, start, &left_path)
                    && self.nested(right, scope.split as usize, &right_path)
            }
        }
    }
}
