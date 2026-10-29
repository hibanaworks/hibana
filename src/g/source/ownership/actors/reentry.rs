//! Construction-only knowledge of resource-dependent reentry. Temporary and
//! permanent resource departures both require permission for another visit.
//! Independent par components stay independent.
//! The workspace below is compiler scratch, never resident protocol state.

use super::super::{
    Arm, At, Atom, Context, EndpointKey, EndpointKind, Evaluation, Message, Payload, Query, Scope,
    Scratch, Selection, Source,
};
use super::{ActorImage, ActorRef};

#[cfg(kani)]
#[path = "reentry/kani.rs"]
mod kani;

#[cfg(all(test, hibana_repo_tests))]
#[path = "reentry/tests.rs"]
mod tests;

pub(super) const fn check<const S: usize, const E: usize, const D: usize>(
    source: &Source<'_>,
    image: &ActorImage<E, D>,
    facts: &Scratch<S, E>,
) -> bool {
    if source.event_count() != image.event_len || facts.event_len != image.event_len {
        panic!("reentry source, ownership facts and actor image disagree");
    }
    Checker {
        root: source,
        image,
        query: Query::new(
            source,
            facts.symbols.split_at(facts.symbol_len).0,
            &facts.initial,
        ),
        messages: facts.events.split_at(facts.event_len).0,
    }
    .nested(source, 0, &Context::new(&[]))
}

struct Checker<'a, const E: usize, const D: usize> {
    root: &'a Source<'a>,
    image: &'a ActorImage<E, D>,
    query: Query<'a>,
    messages: &'a [Option<Message>],
}

struct Body<'a> {
    source: &'a Source<'a>,
    start: usize,
    key: EndpointKey,
}

#[cfg(any(kani, all(test, hibana_repo_tests)))]
const fn wire_events<const E: usize>(source: &Source<'_>) -> [Option<Message>; E] {
    assert!(!source.has_delegation() && source.event_count() <= E);
    let mut messages = [None; E];
    let mut event = 0;
    while event < source.event_count() {
        messages[event] = Some(source.message_at(event));
        event += 1;
    }
    messages
}

impl<const E: usize, const D: usize> Checker<'_, E, D> {
    #[cfg(any(kani, all(test, hibana_repo_tests)))]
    const fn wire_fixture<'a>(
        source: &'a Source<'a>,
        image: &'a ActorImage<E, D>,
        messages: &'a [Option<Message>],
    ) -> Checker<'a, E, D> {
        assert!(!source.has_delegation());
        Checker {
            root: source,
            image,
            query: Query::new(source, &[], &[]),
            messages,
        }
    }

    const fn message_at(&self, event: usize) -> Message {
        match self.messages[event] {
            Some(message) => message,
            None => panic!("checked source message is missing"),
        }
    }

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
        match self.query.owner(At { key, cut }, path) {
            Evaluation::Atom(Atom::Actor(actor)) => Ok(actor),
            Evaluation::Choice(scope) => Err(scope),
            Evaluation::Atom(Atom::Initial(_)) => panic!("unresolved checked initial owner"),
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
            Source::Seq(left, right, _) | Source::Par(left, right, _) => {
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
            Source::Roll(body, _) => Self::present(body, start, event, path),
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
            return self.temporary_departures(body, owner, path);
        }
        self.origins(body, owner, path)
    }

    const fn transfer(
        &self,
        event: usize,
        key: EndpointKey,
        path: &Context<'_>,
    ) -> Result<Option<(u8, u8)>, Scope> {
        if !matches!(self.message_at(event).payload,
            Payload::Endpoint(payload) if payload.same(key))
        {
            return Ok(None);
        }
        match Self::present(self.root, 0, event, path) {
            Ok(false) => return Ok(None),
            Ok(true) => (),
            Err(scope) => return Err(scope),
        }
        let sender = match self.side(event, 0, path) {
            Ok(actor) => actor,
            Err(scope) => return Err(scope),
        };
        match self.side(event, 1, path) {
            Ok(receiver) => Ok(Some((sender, receiver))),
            Err(scope) => Err(scope),
        }
    }

    /// A temporary absence is harmless when its return can proceed without
    /// resources moved by a fresh visit. Only irreversible sends that can
    /// obstruct that return need fresh permission; ordinary queued messages
    /// may wait for a return through an independent logical view.
    const fn temporary_departures(
        &self,
        body: &Body<'_>,
        owner: u8,
        path: &Context<'_>,
    ) -> Result<bool, Scope> {
        let end = body.start + body.source.event_count();
        let mut needed = [0; E];
        let mut event = end;
        while event < self.image.event_len {
            match self.transfer(event, body.key, path) {
                Ok(Some((sender, receiver))) if sender == owner && receiver != owner => {
                    let returned = match self.return_after(body.key, event, owner, path) {
                        Ok(Some(returned)) => returned,
                        Ok(None) => return Ok(false),
                        Err(scope) => return Err(scope),
                    };
                    let mut returning = [0; E];
                    returning[returned] = 2;
                    match self.close_dependencies_after(self.root, 0, event, &mut returning, path) {
                        Ok(()) => (),
                        Err(scope) => return Err(scope),
                    }
                    let mut outgoing = body.start;
                    while outgoing < end {
                        if let Payload::Endpoint(key) = self.message_at(outgoing).payload
                            && !key.same(body.key)
                        {
                            let present =
                                match Self::present(body.source, body.start, outgoing, path) {
                                    Ok(present) => present,
                                    Err(scope) => return Err(scope),
                                };
                            let mut prior = event;
                            while present && prior <= returned {
                                let message = self.message_at(prior);
                                let uses = (message.from.same(key) && returning[prior] & 1 != 0)
                                    || (message.to.same(key) && returning[prior] & 2 != 0)
                                    || (matches!(message.payload, Payload::Endpoint(payload) if payload.same(key))
                                        && returning[prior] != 0);
                                if uses {
                                    needed[outgoing] = 1;
                                    break;
                                }
                                prior += 1;
                            }
                        }
                        outgoing += 1;
                    }
                }
                Ok(_) => (),
                Err(scope) => return Err(scope),
            }
            event += 1;
        }
        self.permitted_origins(body, owner, needed, path)
    }

    const fn return_after(
        &self,
        key: EndpointKey,
        departure: usize,
        owner: u8,
        path: &Context<'_>,
    ) -> Result<Option<usize>, Scope> {
        let mut event = departure + 1;
        while event < self.image.event_len {
            match self.transfer(event, key, path) {
                Ok(Some((_, receiver))) if receiver == owner => return Ok(Some(event)),
                Ok(_) => (),
                Err(scope) => return Err(scope),
            }
            event += 1;
        }
        Ok(None)
    }

    const fn causally_before(
        &self,
        first: (usize, usize),
        second: (usize, usize),
        path: &Context<'_>,
    ) -> Result<bool, Scope> {
        if first.0 > second.0 || (first.0 == second.0 && first.1 >= second.1) {
            return Ok(false);
        }
        let mut needed = [0; E];
        needed[second.0] = 1 << second.1;
        match self.close_dependencies_after(self.root, 0, first.0, &mut needed, path) {
            Ok(()) => Ok(needed[first.0] & (1 << first.1) != 0),
            Err(scope) => Err(scope),
        }
    }

    /// A local entry resource can guard a remote resource's entire absence.
    /// Its send precedes the remote departure; restoring it follows acceptance
    /// of the remote return. That accepted return can then be received without
    /// another visit. Preorder alone supplies neither causal edge.
    const fn guarded_excursion(
        &self,
        body: &Body<'_>,
        actor: u8,
        departure: usize,
        returned: Option<usize>,
        path: &Context<'_>,
    ) -> Result<bool, Scope> {
        let mut guard = body.start + body.source.event_count();
        while guard < departure {
            if let Payload::Endpoint(key) = self.message_at(guard).payload
                && !key.same(body.key)
                && body.source.uses(key)
            {
                let owner = match self.owner(key, body.start, path) {
                    Ok(owner) => owner,
                    Err(scope) => return Err(scope),
                };
                if owner == actor {
                    match self.transfer(guard, key, path) {
                        Ok(Some((sender, receiver))) if sender == actor && receiver != actor => {
                            let begins =
                                match self.causally_before((guard, 0), (departure, 0), path) {
                                    Ok(begins) => begins,
                                    Err(scope) => return Err(scope),
                                };
                            if begins {
                                let guard_return = match self.return_after(key, guard, actor, path)
                                {
                                    Ok(event) => event,
                                    Err(scope) => return Err(scope),
                                };
                                match (returned, guard_return) {
                                    (_, None) => return Ok(true),
                                    (Some(returned), Some(guard_return)) => {
                                        match self.causally_before(
                                            (returned, 0),
                                            (guard_return, 1),
                                            path,
                                        ) {
                                            Ok(true) => return Ok(true),
                                            Ok(false) => (),
                                            Err(scope) => return Err(scope),
                                        }
                                    }
                                    (None, Some(_)) => (),
                                }
                            }
                        }
                        Ok(_) => (),
                        Err(scope) => return Err(scope),
                    }
                }
            }
            guard += 1;
        }
        Ok(false)
    }

    const fn guarded_departures(
        &self,
        body: &Body<'_>,
        owner: u8,
        actor: u8,
        path: &Context<'_>,
    ) -> Result<bool, Scope> {
        let mut found = false;
        let mut event = body.start + body.source.event_count();
        while event < self.image.event_len {
            match self.transfer(event, body.key, path) {
                Ok(Some((sender, receiver))) if sender == owner && receiver != owner => {
                    found = true;
                    let returned = match self.return_after(body.key, event, owner, path) {
                        Ok(event) => event,
                        Err(scope) => return Err(scope),
                    };
                    match self.guarded_excursion(body, actor, event, returned, path) {
                        Ok(true) => (),
                        other => return other,
                    }
                }
                Ok(_) => (),
                Err(scope) => return Err(scope),
            }
            event += 1;
        }
        Ok(found)
    }

    const fn origins(&self, body: &Body<'_>, owner: u8, path: &Context<'_>) -> Result<bool, Scope> {
        // Two bits per source event denote its send and receive actions. Seed
        // resource uses, then follow only genuine actor-local and wire edges
        // backwards. Preorder is a topological enumeration, not a dependency.
        let mut needed = [0u8; E];
        let end = body.start + body.source.event_count();
        let mut event = body.start;
        while event < end {
            let message = self.message_at(event);
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
        self.permitted_origins(body, owner, needed, path)
    }

    const fn permitted_origins(
        &self,
        body: &Body<'_>,
        owner: u8,
        mut needed: [u8; E],
        path: &Context<'_>,
    ) -> Result<bool, Scope> {
        match self.close_dependencies(body.source, body.start, &mut needed, path) {
            Ok(()) => (),
            Err(scope) => return Err(scope),
        }

        // Each origin either observes an incoming permission in this visit,
        // owns the resource, or owns a resource covering its remote absence.
        let mut event = body.start;
        let end = body.start + body.source.event_count();
        while event < end {
            if needed[event] & 1 != 0 {
                let actor = match self.side(event, 0, path) {
                    Ok(actor) => actor,
                    Err(choice) => return Err(choice),
                };
                if actor != owner {
                    let mut informed = false;
                    let mut lower = 0;
                    while !informed {
                        let Some((first, end)) =
                            body.source.predecessor_range(event - body.start, lower)
                        else {
                            break;
                        };
                        let mut prior = body.start + first;
                        while prior < body.start + end && !informed {
                            if needed[prior] & 2 != 0 {
                                match self.side(prior, 1, path) {
                                    Ok(receiver) => informed = receiver == actor,
                                    Err(choice) => return Err(choice),
                                }
                            }
                            prior += 1;
                        }
                        lower = end;
                    }
                    if !informed {
                        match self.guarded_departures(body, owner, actor, path) {
                            Ok(true) => (),
                            other => return other,
                        }
                    }
                }
            }
            event += 1;
        }
        Ok(true)
    }

    /// Close this visit's action dependencies with the original source order.
    const fn close_dependencies(
        &self,
        source: &Source<'_>,
        start: usize,
        needed: &mut [u8; E],
        path: &Context<'_>,
    ) -> Result<(), Scope> {
        self.close_dependencies_after(source, start, start, needed, path)
    }

    /// Queries about a suffix need no earlier predecessor. Source dependencies
    /// strictly increase event position, so a path cannot leave the suffix and
    /// return. Keeping the lower bound also avoids expanding irrelevant choices.
    const fn close_dependencies_after(
        &self,
        source: &Source<'_>,
        start: usize,
        floor: usize,
        needed: &mut [u8; E],
        path: &Context<'_>,
    ) -> Result<(), Scope> {
        let begin = if floor > start { floor } else { start };
        let mut event = start + source.event_count();
        while event > begin && needed[event - 1] == 0 {
            event -= 1;
        }
        let limit = event;
        let mut remaining = [0u32; E];
        let mut index = limit;
        while index > begin {
            index -= 1;
            if needed[index] != 3 {
                remaining[index / 32] |= 1 << (index % 32);
            }
        }
        while event > begin {
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
                    let mut lower = begin - start;
                    while let Some((first, end)) = source.predecessor_range(event - start, lower) {
                        let mut prior = Self::next_remaining(&remaining, start + first, limit);
                        while prior < start + end {
                            let mut previous_side = 0;
                            while previous_side < 2 {
                                if needed[prior] & (1 << previous_side) == 0 {
                                    let previous = match self.side(prior, previous_side, path) {
                                        Ok(actor) => actor,
                                        Err(choice) => return Err(choice),
                                    };
                                    if previous == actor {
                                        match Self::present(source, start, prior, path) {
                                            Ok(true) => needed[prior] |= 1 << previous_side,
                                            Ok(false) => (),
                                            Err(choice) => return Err(choice),
                                        }
                                    }
                                }
                                previous_side += 1;
                            }
                            if needed[prior] == 3 {
                                remaining[prior / 32] &= !(1 << (prior % 32));
                            }
                            prior = Self::next_remaining(&remaining, prior + 1, limit);
                        }
                        lower = end;
                    }
                }
                side += 1;
            }
        }

        Ok(())
    }

    /// Compiler-only bits skip actions whose two facets are already included.
    /// A word lookup has no mutable successor chains to chase or compress.
    const fn next_remaining(bits: &[u32; E], first: usize, limit: usize) -> usize {
        if first >= limit {
            return limit;
        }
        let mut word = first / 32;
        let mut available = bits[word] & (u32::MAX << (first % 32));
        loop {
            if available != 0 {
                let next = word * 32 + available.trailing_zeros() as usize;
                return if next < limit { next } else { limit };
            }
            word += 1;
            if word * 32 >= limit {
                return limit;
            }
            available = bits[word];
        }
    }

    #[cfg(any(kani, all(test, hibana_repo_tests)))]
    const fn reference_close_dependencies(
        &self,
        source: &Source<'_>,
        start: usize,
        needed: &mut [u8; E],
        path: &Context<'_>,
    ) -> Result<(), Scope> {
        let mut event = start + source.event_count();
        while event > start {
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
                    let mut prior = start;
                    while prior < event {
                        if source.before(prior - start, event - start) {
                            let mut previous_side = 0;
                            while previous_side < 2 {
                                let previous = match self.side(prior, previous_side, path) {
                                    Ok(actor) => actor,
                                    Err(choice) => return Err(choice),
                                };
                                if previous == actor {
                                    match Self::present(source, start, prior, path) {
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

        Ok(())
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
            Source::Seq(left, right, _) | Source::Par(left, right, _) => {
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
            Source::Roll(inner, _) => {
                // Collection already contains each complete resource key once.
                // Reentry checks quantify over those keys, not every ordinary
                // message followed by another search for its first transfer.
                let mut index = 0;
                while index < self.query.symbols.len() {
                    let Some(symbol) = self.query.symbols[index] else {
                        panic!("collected reentry resource");
                    };
                    if matches!(symbol.kind, EndpointKind::Delegated)
                        && inner.uses(symbol.key)
                        && !self.body(
                            &Body {
                                source: inner,
                                start,
                                key: symbol.key,
                            },
                            path,
                        )
                    {
                        return false;
                    }
                    index += 1;
                }
                self.nested(inner, start, path)
            }
        }
    }
}
