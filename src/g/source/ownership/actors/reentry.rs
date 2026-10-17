//! Construction-only knowledge of resource-dependent reentry. A roll that
//! permanently gives up an entry resource must let that owner control every
//! causal origin of another visit. Independent par components stay independent.
//! The workspace below is compiler scratch, never resident protocol state.

use super::super::{
    Arm, At, Atom, Context, EndpointKey, Evaluation, Payload, Query, Scope, Selection, Source,
};
use super::{ActorImage, ActorRef};

#[cfg(kani)]
#[path = "reentry/kani.rs"]
mod kani;

pub(super) const fn check<const E: usize, const D: usize>(
    source: &Source<'_>,
    image: &ActorImage<E, D>,
) -> bool {
    if source.event_count() != image.event_len {
        panic!("reentry source and actor image disagree");
    }
    Checker {
        root: source,
        image,
    }
    .nested(source, 0, &Context::new(&[]))
}

struct Checker<'a, const E: usize, const D: usize> {
    root: &'a Source<'a>,
    image: &'a ActorImage<E, D>,
}

struct Body<'a> {
    source: &'a Source<'a>,
    start: usize,
    key: EndpointKey,
}

impl<const E: usize, const D: usize> Checker<'_, E, D> {
    const fn actor(&self, mut reference: ActorRef, path: &Context<'_>) -> Result<u8, Scope> {
        while reference.0 >= 256 {
            let choice = self.image.choices[reference.0 as usize - 256];
            reference = match path.get(choice.scope) {
                Some(Arm::Left) => choice.left,
                Some(Arm::Right) => choice.right,
                None => return Err(choice.scope),
            };
        }
        Ok(reference.0 as u8)
    }

    const fn owner(&self, key: EndpointKey, cut: usize, path: &Context<'_>) -> Result<u8, Scope> {
        match Query::new(self.root).owner(At { key, cut }, path) {
            Evaluation::Atom(Atom::Actor(actor)) => Ok(actor),
            Evaluation::Choice(scope) => Err(scope),
            Evaluation::Atom(Atom::Initial(key)) => {
                let mut event = 0;
                while event < self.image.event_len {
                    let message = self.root.message_at(event);
                    if message.from.same(key)
                        || matches!(message.payload, Payload::Endpoint(payload) if payload.same(key))
                    {
                        return self.actor(self.image.events[event].sender, path);
                    }
                    if message.to.same(key) {
                        return self.actor(self.image.events[event].receiver, path);
                    }
                    event += 1;
                }
                panic!("initial resource has no source occurrence");
            }
        }
    }

    const fn present(
        source: &Source<'_>,
        start: usize,
        event: usize,
        path: &Context<'_>,
    ) -> Result<bool, Scope> {
        match source {
            Source::Send(_) => Ok(true),
            Source::Seq(left, right) | Source::Par(left, right) => {
                let split = start + left.event_count();
                if event < split {
                    Self::present(left, start, event, path)
                } else {
                    Self::present(right, split, event, path)
                }
            }
            Source::Route { left, right, .. } => {
                let split = start + left.event_count();
                let scope = Scope {
                    start: start as u16,
                    split: split as u16,
                    stop: (split + right.event_count()) as u16,
                };
                match path.get(scope) {
                    None => Err(scope),
                    Some(Arm::Left) if event < split => Self::present(left, start, event, path),
                    Some(Arm::Right) if event >= split => Self::present(right, split, event, path),
                    Some(_) => Ok(false),
                }
            }
            Source::Roll(body) => Self::present(body, start, event, path),
        }
    }

    const fn side(&self, event: usize, side: usize, path: &Context<'_>) -> Result<u8, Scope> {
        let pair = self.image.events[event];
        self.actor(
            if side == 0 {
                pair.sender
            } else {
                pair.receiver
            },
            path,
        )
    }

    const fn run(&self, body: &Body<'_>, path: &Context<'_>) -> Result<bool, Scope> {
        let owner = match self.owner(body.key, body.start, path) {
            Ok(actor) => actor,
            Err(choice) => return Err(choice),
        };
        let final_owner = match self.owner(body.key, self.image.event_len, path) {
            Ok(actor) => actor,
            Err(choice) => return Err(choice),
        };
        if owner == final_owner {
            return Ok(true);
        }
        self.origins(body, owner, path)
    }

    const fn origins(&self, body: &Body<'_>, owner: u8, path: &Context<'_>) -> Result<bool, Scope> {
        // Two bits per source event denote its send and receive actions. Seed
        // resource uses, then follow only genuine actor-local and wire edges
        // backwards. Preorder is a topological enumeration, not a dependency.
        let mut needed = [0u8; E];
        let end = body.start + body.source.event_count();
        let mut event = body.start;
        while event < end {
            let message = self.root.message_at(event);
            if message.from.same(body.key)
                || message.to.same(body.key)
                || matches!(message.payload, Payload::Endpoint(key) if key.same(body.key))
            {
                match Self::present(body.source, body.start, event, path) {
                    Ok(true) => needed[event] = 3,
                    Ok(false) => (),
                    Err(choice) => return Err(choice),
                }
            }
            event += 1;
        }
        event = end;
        while event > body.start {
            event -= 1;
            if needed[event] & 2 != 0 {
                needed[event] |= 1;
            }
            let mut side = 0;
            while side < 2 {
                if needed[event] & (1 << side) != 0 {
                    let actor = match self.side(event, side, path) {
                        Ok(actor) => actor,
                        Err(choice) => return Err(choice),
                    };
                    let mut prior = body.start;
                    while prior < event {
                        if body.source.before(prior - body.start, event - body.start) {
                            let mut previous_side = 0;
                            while previous_side < 2 {
                                let previous = match self.side(prior, previous_side, path) {
                                    Ok(actor) => actor,
                                    Err(choice) => return Err(choice),
                                };
                                if previous == actor {
                                    match Self::present(body.source, body.start, prior, path) {
                                        Ok(true) => needed[prior] |= 1 << previous_side,
                                        Ok(false) => (),
                                        Err(choice) => return Err(choice),
                                    }
                                }
                                previous_side += 1;
                            }
                        }
                        prior += 1;
                    }
                }
                side += 1;
            }
        }

        // Every relevant send is rooted at the entry owner, or follows an
        // incoming action in this visit. That incoming action's sender is also
        // in the closure, so finite source order prevents circular permission.
        event = body.start;
        while event < end {
            if needed[event] & 1 != 0 {
                let actor = match self.side(event, 0, path) {
                    Ok(actor) => actor,
                    Err(choice) => return Err(choice),
                };
                if actor != owner {
                    let mut informed = false;
                    let mut prior = body.start;
                    while prior < event && !informed {
                        if needed[prior] & 2 != 0
                            && body.source.before(prior - body.start, event - body.start)
                        {
                            match self.side(prior, 1, path) {
                                Ok(receiver) => informed = receiver == actor,
                                Err(choice) => return Err(choice),
                            }
                        }
                        prior += 1;
                    }
                    if !informed {
                        return Ok(false);
                    }
                }
            }
            event += 1;
        }
        Ok(true)
    }

    const fn body(&self, body: &Body<'_>, path: &Context<'_>) -> bool {
        let scope = match self.run(body, path) {
            Ok(result) => return result,
            Err(scope) => scope,
        };
        self.body(
            body,
            &path.select(Selection {
                scope,
                arm: Arm::Left,
            }),
        ) && self.body(
            body,
            &path.select(Selection {
                scope,
                arm: Arm::Right,
            }),
        )
    }

    const fn nested(&self, source: &Source<'_>, start: usize, path: &Context<'_>) -> bool {
        match source {
            Source::Send(_) => true,
            Source::Seq(left, right) | Source::Par(left, right) => {
                self.nested(left, start, path)
                    && self.nested(right, start + left.event_count(), path)
            }
            Source::Route { left, right, .. } => {
                let split = start + left.event_count();
                let scope = Scope {
                    start: start as u16,
                    split: split as u16,
                    stop: (split + right.event_count()) as u16,
                };
                self.nested(
                    left,
                    start,
                    &path.select(Selection {
                        scope,
                        arm: Arm::Left,
                    }),
                ) && self.nested(
                    right,
                    split,
                    &path.select(Selection {
                        scope,
                        arm: Arm::Right,
                    }),
                )
            }
            Source::Roll(inner) => {
                let mut event = 0;
                while event < self.image.event_len {
                    if let Payload::Endpoint(key) = self.root.message_at(event).payload
                        && inner.uses(key)
                        && !self.body(
                            &Body {
                                source: inner,
                                start,
                                key,
                            },
                            path,
                        )
                    {
                        return false;
                    }
                    event += 1;
                }
                self.nested(inner, start, path)
            }
        }
    }
}
