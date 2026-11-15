//! Construction-only knowledge of resource-dependent reentry. Temporary and
//! permanent resource departures both require permission for another visit.
//! Independent par components stay independent.
//! The workspace below is compiler scratch, never resident protocol state.

use super::super::{
    Arm, At, Atom, Context, EndpointKey, EndpointKind, Evaluation, Message, Payload, Query, Scope,
    Scratch, Selection, Source,
};
use super::{ActorImage, ActorRef};

const ACTOR_COUNT: usize = u8::MAX as usize + 1;
const ACTOR_WORDS: usize = ACTOR_COUNT / u64::BITS as usize;
const ACTOR_ROOTS: [u8; ACTOR_COUNT] = {
    let mut roots = [0; ACTOR_COUNT];
    let mut actor = 0;
    while actor < ACTOR_COUNT {
        roots[actor] = actor as u8;
        actor += 1;
    }
    roots
};

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
    Checker::new(
        source,
        image,
        (
            Query::new(
                source,
                facts.symbols.split_at(facts.symbol_len).0,
                &facts.initial,
            ),
            facts.events.split_at(facts.event_len).0,
        ),
    )
    .nested(source, 0, &Context::new(&[]))
}

#[cfg(hibana_profile)]
pub(super) fn conflict<const S: usize, const E: usize, const D: usize>(
    source: &Source<'_>,
    image: &ActorImage<E, D>,
    facts: &Scratch<S, E>,
) -> Option<(usize, EndpointKey)> {
    let checker = Checker::new(
        source,
        image,
        (
            Query::new(
                source,
                facts.symbols.split_at(facts.symbol_len).0,
                &facts.initial,
            ),
            facts.events.split_at(facts.event_len).0,
        ),
    );
    fn walk<const E: usize, const D: usize>(
        checker: &Checker<'_, E, D>,
        source: &Source<'_>,
        start: usize,
        path: &Context<'_>,
    ) -> Option<(usize, EndpointKey)> {
        match source {
            Source::Send(_) => None,
            Source::Seq(left, right, _) | Source::Par(left, right, _) => {
                walk(checker, left, start, path)
                    .or_else(|| walk(checker, right, start + left.event_count(), path))
            }
            Source::Route { left, right, .. } => {
                let split = start + left.event_count();
                let scope = Scope {
                    start: start as u16,
                    split: split as u16,
                    stop: (split + right.event_count()) as u16,
                };
                walk(
                    checker,
                    left,
                    start,
                    &path.select(Selection {
                        scope,
                        arm: Arm::Left,
                    }),
                )
                .or_else(|| {
                    walk(
                        checker,
                        right,
                        split,
                        &path.select(Selection {
                            scope,
                            arm: Arm::Right,
                        }),
                    )
                })
            }
            Source::Roll(inner, _) => {
                for symbol in checker.query.symbols {
                    let symbol = symbol.as_ref().expect("collected reentry resource");
                    if matches!(symbol.kind, EndpointKind::Delegated)
                        && inner.uses(symbol.key)
                        && !checker.body(
                            &Body {
                                source: inner,
                                start,
                                key: symbol.key,
                            },
                            path,
                        )
                    {
                        return Some((start, symbol.key));
                    }
                }
                walk(checker, inner, start, path)
            }
        }
    }
    walk(&checker, source, 0, &Context::new(&[]))
}

struct Checker<'a, const E: usize, const D: usize> {
    root: &'a Source<'a>,
    image: &'a ActorImage<E, D>,
    query: Query<'a>,
    messages: &'a [Option<Message>],
    event_routes: [u16; E],
    route_parents: [(Scope, u16); E],
    // Source-order Endpoint transfers only. Branch presence and actor binding
    // are still checked at each candidate; this index never selects a path.
    next_transfer: [u16; E],
    next_key_transfer: [u16; E],
    first_transfer: u16,
    actor_bound: usize,
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
    const fn new<'a>(
        source: &'a Source<'a>,
        image: &'a ActorImage<E, D>,
        (query, messages): (Query<'a>, &'a [Option<Message>]),
    ) -> Checker<'a, E, D> {
        let mut checker = Checker {
            root: source,
            image,
            query,
            messages,
            event_routes: [0; E],
            route_parents: [(
                Scope {
                    start: 0,
                    split: 0,
                    stop: 0,
                },
                0,
            ); E],
            next_transfer: [u16::MAX; E],
            next_key_transfer: [u16::MAX; E],
            first_transfer: u16::MAX,
            actor_bound: 1,
        };
        let mut choice = 0;
        while choice < image.choice_len {
            let row = image.choices[choice];
            checker.include_actor_bound(row.left);
            checker.include_actor_bound(row.right);
            choice += 1;
        }
        checker.index_routes(source, 0, 0);
        let mut heads = [None; E];
        let mut head_len = 0;
        let mut event = image.event_len;
        while event > 0 {
            event -= 1;
            checker.include_actor_bound(image.events[event].sender);
            checker.include_actor_bound(image.events[event].receiver);
            checker.next_transfer[event] = checker.first_transfer;
            if let Payload::Endpoint(key) = checker.message_at(event).payload {
                checker.first_transfer = event as u16;
                let mut slot = 0;
                while slot < head_len {
                    match heads[slot] {
                        Some((previous_key, next)) if key.same(previous_key) => {
                            checker.next_key_transfer[event] = next;
                            break;
                        }
                        Some(_) => slot += 1,
                        None => panic!("indexed transfer key"),
                    }
                }
                if slot == head_len {
                    head_len += 1;
                }
                heads[slot] = Some((key, event as u16));
            }
        }
        checker
    }

    const fn include_actor_bound(&mut self, reference: ActorRef) {
        if reference.0 < 256 && reference.0 as usize >= self.actor_bound {
            self.actor_bound = reference.0 as usize + 1;
        }
    }

    const fn transfers_from(&self, cut: usize) -> usize {
        if cut == 0 {
            self.first_transfer as usize
        } else {
            self.next_transfer[cut - 1] as usize
        }
    }

    const fn key_transfers_from(&self, key: EndpointKey, cut: usize) -> usize {
        let mut event = self.transfers_from(cut);
        while event < self.image.event_len {
            if matches!(self.message_at(event).payload,
                Payload::Endpoint(payload) if payload.same(key))
            {
                break;
            }
            event = self.next_transfer[event] as usize;
        }
        event
    }

    // Binary splits are unique and nonzero. Store only immutable route
    // ancestry, so membership queries skip every intervening seq/par node.
    // This workspace is discarded after construction, not emitted or mutable
    // protocol state. An unresolved outer route still precedes its children.
    const fn index_routes(&mut self, source: &Source<'_>, start: usize, parent: u16) {
        match source {
            Source::Send(_) => self.event_routes[start] = parent,
            Source::Seq(left, right, _) | Source::Par(left, right, _) => {
                self.index_routes(left, start, parent);
                self.index_routes(right, start + left.event_count(), parent);
            }
            Source::Route { left, right, .. } => {
                let split = start + left.event_count();
                let scope = Scope {
                    start: start as u16,
                    split: split as u16,
                    stop: (split + right.event_count()) as u16,
                };
                self.route_parents[split] = (scope, parent);
                self.index_routes(left, start, split as u16);
                self.index_routes(right, split, split as u16);
            }
            Source::Roll(body, _) => self.index_routes(body, start, parent),
        }
    }

    const fn present_at(&self, event: usize, path: &Context<'_>) -> Result<bool, Scope> {
        self.present_under(self.event_routes[event], event, path)
    }

    const fn present_under(
        &self,
        route: u16,
        event: usize,
        path: &Context<'_>,
    ) -> Result<bool, Scope> {
        if route == 0 {
            return Ok(true);
        }
        let (scope, parent) = self.route_parents[route as usize];
        match self.present_under(parent, event, path) {
            Ok(true) => (),
            other => return other,
        }
        match path.get(scope) {
            None => Err(scope),
            Some(Arm::Left) => Ok(event < scope.split as usize),
            Some(Arm::Right) => Ok(event >= scope.split as usize),
        }
    }

    #[cfg(any(kani, all(test, hibana_repo_tests)))]
    const fn wire_fixture<'a>(
        source: &'a Source<'a>,
        image: &'a ActorImage<E, D>,
        messages: &'a [Option<Message>],
    ) -> Checker<'a, E, D> {
        assert!(!source.has_delegation());
        Checker::new(source, image, (Query::new(source, &[], &[]), messages))
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

    #[cfg(any(kani, all(test, hibana_repo_tests)))]
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

    const fn run(
        &self,
        body: &Body<'_>,
        path: &Context<'_>,
        owners: &mut (Option<u8>, Option<u8>),
    ) -> Result<bool, Scope> {
        let owner = match owners.0 {
            Some(actor) => actor,
            None => match self.owner(body.key, body.start, path) {
                Ok(actor) => {
                    owners.0 = Some(actor);
                    actor
                }
                Err(choice) => return Err(choice),
            },
        };
        let final_owner = match owners.1 {
            Some(actor) => actor,
            None => match self.owner(body.key, self.image.event_len, path) {
                Ok(actor) => {
                    owners.1 = Some(actor);
                    actor
                }
                Err(choice) => return Err(choice),
            },
        };
        if owner == final_owner {
            let mut event = self.transfers_from(body.start);
            while event < body.start + body.source.event_count() {
                if matches!(self.message_at(event).payload,
                    Payload::Endpoint(key) if !key.same(body.key))
                {
                    return self.temporary_departures(body, owner, path);
                }
                event = self.next_transfer[event] as usize;
            }
            // Validated ownership ends at the entry owner, so every later
            // departure has a source return. Without another outgoing endpoint this
            // body cannot seed any obligation obstructing those returns.
            return Ok(true);
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
        match self.present_at(event, path) {
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
        let mut event = self.key_transfers_from(body.key, end);
        while event < self.image.event_len {
            let pair = self.image.events[event];
            if (pair.sender.0 < 256 && pair.sender.0 != owner as u16)
                || pair.receiver.0 == owner as u16
            {
                event = self.next_key_transfer[event] as usize;
                continue;
            }
            match self.transfer(event, body.key, path) {
                Ok(Some((sender, receiver))) if sender == owner && receiver != owner => {
                    let returned = match self.return_after(body.key, event, owner, path) {
                        Ok(Some(returned)) => returned,
                        Ok(None) => return Ok(false),
                        Err(scope) => return Err(scope),
                    };
                    // With no Endpoint sends in the body, no newly moved
                    // resource can obstruct this already located return.
                    if !body.source.has_delegation() {
                        event = self.next_key_transfer[event] as usize;
                        continue;
                    }
                    // No dependency closure can mark a use that is absent
                    // from the complete source interval. Keep finding the
                    // actual return first; only its possible obstruction is
                    // pruned, irrespective of still-unselected route arms.
                    if !self.outgoing_resource_used_between(body, (event, returned)) {
                        event = self.next_key_transfer[event] as usize;
                        continue;
                    }
                    let mut returning = [0; E];
                    returning[returned] = 2;
                    match self.close_dependencies_after(self.root, 0, event, &mut returning, path) {
                        Ok(()) => (),
                        Err(scope) => return Err(scope),
                    }
                    let mut outgoing = self.transfers_from(body.start);
                    while outgoing < end {
                        if let Payload::Endpoint(key) = self.message_at(outgoing).payload
                            && !key.same(body.key)
                        {
                            let present = match self.present_at(outgoing, path) {
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
                        outgoing = self.next_transfer[outgoing] as usize;
                    }
                }
                Ok(_) => (),
                Err(scope) => return Err(scope),
            }
            event = self.next_key_transfer[event] as usize;
        }
        self.permitted_origins(body, owner, needed, path)
    }

    const fn outgoing_resource_used_between(
        &self,
        body: &Body<'_>,
        (begin, end): (usize, usize),
    ) -> bool {
        let stop = body.start + body.source.event_count();
        let mut outgoing = self.transfers_from(body.start);
        while outgoing < stop {
            if let Payload::Endpoint(key) = self.message_at(outgoing).payload
                && !key.same(body.key)
            {
                let mut event = begin;
                while event <= end {
                    let message = self.message_at(event);
                    if message.from.same(key)
                        || message.to.same(key)
                        || matches!(message.payload, Payload::Endpoint(payload) if payload.same(key))
                    {
                        return true;
                    }
                    event += 1;
                }
            }
            outgoing = self.next_transfer[outgoing] as usize;
        }
        false
    }

    const fn return_after(
        &self,
        key: EndpointKey,
        departure: usize,
        owner: u8,
        path: &Context<'_>,
    ) -> Result<Option<usize>, Scope> {
        let mut event = self.key_transfers_from(key, departure + 1);
        while event < self.image.event_len {
            let receiver = self.image.events[event].receiver;
            if receiver.0 < 256 && receiver.0 != owner as u16 {
                event = self.next_key_transfer[event] as usize;
                continue;
            }
            match self.transfer(event, key, path) {
                Ok(Some((_, receiver))) if receiver == owner => return Ok(Some(event)),
                Ok(_) => (),
                Err(scope) => return Err(scope),
            }
            event = self.next_key_transfer[event] as usize;
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
        // A source-ordered local edge, including either message's send/receive
        // edge, already witnesses reachability. Do not construct its entire
        // transitive closure. Unknown presence/binding falls through unchanged.
        if matches!(self.present_at(first.0, path), Ok(true))
            && matches!(self.present_at(second.0, path), Ok(true))
            && self.root.before(first.0, second.0)
        {
            if let (Ok(left), Ok(right)) = (
                self.side(first.0, first.1, path),
                self.side(second.0, second.1, path),
            ) && (left == right
                || (first.1 == 0
                    && matches!(self.side(first.0, 1, path), Ok(actor) if actor == right))
                || (second.1 == 1
                    && matches!(self.side(second.0, 0, path), Ok(actor) if actor == left)))
            {
                return Ok(true);
            }
            if first.1 == 0
                && second.1 == 1
                && let (Ok(left), Ok(right)) =
                    (self.side(first.0, 1, path), self.side(second.0, 0, path))
                && left == right
            {
                return Ok(true);
            }
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
        let floor = body.start + body.source.event_count();
        let mut guard = self.transfers_from(floor);
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
            guard = self.next_transfer[guard] as usize;
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
        let mut event = self.key_transfers_from(body.key, body.start + body.source.event_count());
        while event < self.image.event_len {
            let pair = self.image.events[event];
            if (pair.sender.0 < 256 && pair.sender.0 != owner as u16)
                || pair.receiver.0 == owner as u16
            {
                event = self.next_key_transfer[event] as usize;
                continue;
            }
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
            event = self.next_key_transfer[event] as usize;
        }
        Ok(found)
    }

    const fn origins(&self, body: &Body<'_>, owner: u8, path: &Context<'_>) -> Result<bool, Scope> {
        let mut needed = [0u8; E];
        let mut actors = [0u16; E];
        let mut parents = ACTOR_ROOTS;
        let mut connected = [false; ACTOR_COUNT];
        match self.origins_graph(
            (body.source, body.start),
            body.key,
            (&mut parents, &mut connected),
            (&mut needed, &mut actors),
            path,
        ) {
            Ok(()) => (),
            Err(scope) => return Err(scope),
        }
        let mut event = body.start;
        let end = body.start + body.source.event_count();
        while event < end {
            if needed[event] != 0 {
                let root = Self::component_root(&mut parents, (actors[event] >> 8) as u8);
                needed[event] = if connected[root as usize] { 3 } else { 0 };
            }
            event += 1;
        }
        // Every action in an included actor component is already marked on
        // both sides. Actor-ordered predecessors cannot add another component,
        // so repeating the general dependency closure changes no needed bit.
        self.permission_prefix(
            (body.source, body.start),
            &needed,
            (&mut [0; ACTOR_WORDS], &mut [0; ACTOR_WORDS]),
            (body, owner),
            path,
        )
    }

    const fn component_root(parents: &mut [u8; ACTOR_COUNT], mut actor: u8) -> u8 {
        let mut root = actor;
        while parents[root as usize] != root {
            root = parents[root as usize];
        }
        while actor != root {
            let next = parents[actor as usize];
            parents[actor as usize] = root;
            actor = next;
        }
        root
    }

    const fn join_component(
        (parents, connected): (&mut [u8; ACTOR_COUNT], &mut [bool; ACTOR_COUNT]),
        (sender, receiver): (u8, u8),
        seed: bool,
    ) {
        let left = Self::component_root(parents, sender);
        let right = Self::component_root(parents, receiver);
        let marked = seed || connected[left as usize] || connected[right as usize];
        let root = if left < right {
            parents[right as usize] = left;
            left
        } else {
            parents[left as usize] = right;
            right
        };
        connected[root as usize] = marked;
    }

    /// Merge an unresolved choice only when both arms yield the same partition
    /// and seeded components. Actor bindings are resolved under their actual
    /// arm; unequal states request the existing path split rather than joining
    /// possible holders. Cached event pairs belong only to compiler scratch.
    const fn origins_graph(
        &self,
        (source, start): (&Source<'_>, usize),
        key: EndpointKey,
        (parents, connected): (&mut [u8; ACTOR_COUNT], &mut [bool; ACTOR_COUNT]),
        (needed, actors): (&mut [u8; E], &mut [u16; E]),
        path: &Context<'_>,
    ) -> Result<(), Scope> {
        match source {
            Source::Send(_) => {
                let sender = match self.side(start, 0, path) {
                    Ok(actor) => actor,
                    Err(scope) => return Err(scope),
                };
                let receiver = match self.side(start, 1, path) {
                    Ok(actor) => actor,
                    Err(scope) => return Err(scope),
                };
                let message = self.message_at(start);
                let seed = message.from.same(key)
                    || message.to.same(key)
                    || matches!(message.payload, Payload::Endpoint(payload) if payload.same(key));
                actors[start] = (sender as u16) << 8 | receiver as u16;
                needed[start] = 1;
                Self::join_component((parents, connected), (sender, receiver), seed);
                Ok(())
            }
            Source::Seq(left, right, _) | Source::Par(left, right, _) => {
                match self.origins_graph(
                    (left, start),
                    key,
                    (parents, connected),
                    (needed, actors),
                    path,
                ) {
                    Ok(()) => (),
                    Err(scope) => return Err(scope),
                }
                self.origins_graph(
                    (right, start + left.event_count()),
                    key,
                    (parents, connected),
                    (needed, actors),
                    path,
                )
            }
            Source::Route { left, right, .. } => {
                let split = start + left.event_count();
                let scope = Scope {
                    start: start as u16,
                    split: split as u16,
                    stop: (split + right.event_count()) as u16,
                };
                match path.get(scope) {
                    Some(Arm::Left) => self.origins_graph(
                        (left, start),
                        key,
                        (parents, connected),
                        (needed, actors),
                        path,
                    ),
                    Some(Arm::Right) => self.origins_graph(
                        (right, split),
                        key,
                        (parents, connected),
                        (needed, actors),
                        path,
                    ),
                    None => {
                        let mut left_parents = *parents;
                        let mut left_connected = *connected;
                        match self.origins_graph(
                            (left, start),
                            key,
                            (&mut left_parents, &mut left_connected),
                            (needed, actors),
                            &path.select(Selection {
                                scope,
                                arm: Arm::Left,
                            }),
                        ) {
                            Ok(()) => (),
                            Err(scope) => return Err(scope),
                        }
                        match self.origins_graph(
                            (right, split),
                            key,
                            (parents, connected),
                            (needed, actors),
                            &path.select(Selection {
                                scope,
                                arm: Arm::Right,
                            }),
                        ) {
                            Ok(()) => (),
                            Err(scope) => return Err(scope),
                        }
                        let mut actor = 0;
                        while actor < self.actor_bound {
                            let a = Self::component_root(&mut left_parents, actor as u8);
                            let b = Self::component_root(parents, actor as u8);
                            if a != b || left_connected[a as usize] != connected[b as usize] {
                                return Err(scope);
                            }
                            actor += 1;
                        }
                        Ok(())
                    }
                }
            }
            Source::Roll(inner, _) => self.origins_graph(
                (inner, start),
                key,
                (parents, connected),
                (needed, actors),
                path,
            ),
        }
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

        self.permission_prefix(
            (body.source, body.start),
            &needed,
            (&mut [0; ACTOR_WORDS], &mut [0; ACTOR_WORDS]),
            (body, owner),
            path,
        )
    }

    // Every needed send must own its entry authority, observe an earlier
    // needed receive, or hold a causal guard over the resource's absence.
    // Seq carries receive facts forward; par never lends a sibling's facts
    // before the join. Checking a send precedes recording its own receive.
    const fn permission_event(
        &self,
        event: usize,
        needed: u8,
        (incoming, guards): (&mut [u64; ACTOR_WORDS], &mut [u64; ACTOR_WORDS]),
        (body, owner): (&Body<'_>, u8),
        path: &Context<'_>,
    ) -> Result<bool, Scope> {
        if needed & 1 != 0 {
            let actor = match self.side(event, 0, path) {
                Ok(actor) => actor,
                Err(scope) => return Err(scope),
            };
            if actor != owner
                && incoming[actor as usize / 64] & (1 << (actor % 64)) == 0
                && guards[actor as usize / 64] & (1 << (actor % 64)) == 0
            {
                match self.guarded_departures(body, owner, actor, path) {
                    Ok(true) => guards[actor as usize / 64] |= 1 << (actor % 64),
                    other => return other,
                }
            }
        }
        if needed & 2 != 0 {
            let actor = match self.side(event, 1, path) {
                Ok(actor) => actor,
                Err(scope) => return Err(scope),
            };
            incoming[actor as usize / 64] |= 1 << (actor % 64);
        }
        Ok(true)
    }

    const fn permission_prefix(
        &self,
        (source, start): (&Source<'_>, usize),
        needed: &[u8; E],
        (incoming, guards): (&mut [u64; ACTOR_WORDS], &mut [u64; ACTOR_WORDS]),
        (body, owner): (&Body<'_>, u8),
        path: &Context<'_>,
    ) -> Result<bool, Scope> {
        let stop = start + source.event_count();
        let mut marked = stop;
        while marked > start && needed[marked - 1] == 0 {
            marked -= 1;
        }
        if marked == start {
            return Ok(true);
        }
        if source.is_sequential() {
            let mut event = start;
            while event < marked {
                if needed[event] != 0 {
                    match self.permission_event(
                        event,
                        needed[event],
                        (incoming, guards),
                        (body, owner),
                        path,
                    ) {
                        Ok(true) => (),
                        other => return other,
                    }
                }
                event += 1;
            }
            return Ok(true);
        }
        match source {
            Source::Send(_) => unreachable!(),
            Source::Seq(left, right, _) => {
                match self.permission_prefix(
                    (left, start),
                    needed,
                    (incoming, guards),
                    (body, owner),
                    path,
                ) {
                    Ok(true) => (),
                    other => return other,
                }
                self.permission_prefix(
                    (right, start + left.event_count()),
                    needed,
                    (incoming, guards),
                    (body, owner),
                    path,
                )
            }
            Source::Par(left, right, _) => {
                let mut left_after = *incoming;
                match self.permission_prefix(
                    (left, start),
                    needed,
                    (&mut left_after, guards),
                    (body, owner),
                    path,
                ) {
                    Ok(true) => (),
                    other => return other,
                }
                match self.permission_prefix(
                    (right, start + left.event_count()),
                    needed,
                    (incoming, guards),
                    (body, owner),
                    path,
                ) {
                    Ok(true) => (),
                    other => return other,
                }
                let mut word = 0;
                while word < ACTOR_WORDS {
                    incoming[word] |= left_after[word];
                    word += 1;
                }
                Ok(true)
            }
            Source::Route { left, right, .. } => {
                let split = start + left.event_count();
                let scope = Scope {
                    start: start as u16,
                    split: split as u16,
                    stop: (split + right.event_count()) as u16,
                };
                match path.get(scope) {
                    Some(Arm::Left) => self.permission_prefix(
                        (left, start),
                        needed,
                        (incoming, guards),
                        (body, owner),
                        path,
                    ),
                    Some(Arm::Right) => self.permission_prefix(
                        (right, split),
                        needed,
                        (incoming, guards),
                        (body, owner),
                        path,
                    ),
                    None => {
                        let mut left_incoming = *incoming;
                        let mut left_guards = *guards;
                        match self.permission_prefix(
                            (left, start),
                            needed,
                            (&mut left_incoming, &mut left_guards),
                            (body, owner),
                            &path.select(Selection {
                                scope,
                                arm: Arm::Left,
                            }),
                        ) {
                            Ok(true) => (),
                            other => return other,
                        }
                        match self.permission_prefix(
                            (right, split),
                            needed,
                            (incoming, guards),
                            (body, owner),
                            &path.select(Selection {
                                scope,
                                arm: Arm::Right,
                            }),
                        ) {
                            Ok(true) => (),
                            other => return other,
                        }
                        let mut word = 0;
                        while word < ACTOR_WORDS {
                            if incoming[word] != left_incoming[word]
                                || guards[word] != left_guards[word]
                            {
                                return Err(scope);
                            }
                            word += 1;
                        }
                        Ok(true)
                    }
                }
            }
            Source::Roll(inner, _) => self.permission_prefix(
                (inner, start),
                needed,
                (incoming, guards),
                (body, owner),
                path,
            ),
        }
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

    /// Propagate dependencies backwards through source structure. Sequence
    /// carries actor needs into its prefix; parallel arms receive the same
    /// suffix needs and merge only after both independent arms are processed.
    const fn close_dependencies_after(
        &self,
        source: &Source<'_>,
        start: usize,
        floor: usize,
        needed: &mut [u8; E],
        path: &Context<'_>,
    ) -> Result<(), Scope> {
        let begin = if floor > start { floor } else { start };
        let mut limit = start + source.event_count();
        while limit > begin && needed[limit - 1] == 0 {
            limit -= 1;
        }
        self.dependency_prefix(
            source,
            (start, begin, limit),
            needed,
            &mut [0; ACTOR_WORDS],
            path,
        )
    }

    // A u8 actor domain occupies four u64 words. This is compiler workspace,
    // not protocol progress or resident image storage.
    const fn dependency_event(
        &self,
        event: usize,
        needed: &mut [u8; E],
        incoming: &mut [u64; ACTOR_WORDS],
        path: &Context<'_>,
    ) -> Result<(), Scope> {
        let active = incoming[0] | incoming[1] | incoming[2] | incoming[3] != 0;
        let sender = match self.side(event, 0, path) {
            Ok(actor) => actor,
            Err(scope) => return Err(scope),
        };
        let receiver = if active || needed[event] & 2 != 0 {
            match self.side(event, 1, path) {
                Ok(actor) => Some(actor),
                Err(scope) => return Err(scope),
            }
        } else {
            None
        };
        let mut bits = needed[event];
        if incoming[sender as usize / 64] & (1 << (sender % 64)) != 0 {
            bits |= 1;
        }
        if let Some(receiver) = receiver
            && incoming[receiver as usize / 64] & (1 << (receiver % 64)) != 0
        {
            bits |= 2;
        }
        if bits & 2 != 0 {
            bits |= 1;
        }
        needed[event] = bits;
        if bits & 1 != 0 {
            incoming[sender as usize / 64] |= 1 << (sender % 64);
        }
        if bits & 2 != 0 {
            match receiver {
                Some(actor) => incoming[actor as usize / 64] |= 1 << (actor % 64),
                None => panic!("needed receive has no actor"),
            }
        }
        Ok(())
    }

    const fn dependency_prefix(
        &self,
        source: &Source<'_>,
        (start, floor, limit): (usize, usize, usize),
        needed: &mut [u8; E],
        incoming: &mut [u64; ACTOR_WORDS],
        path: &Context<'_>,
    ) -> Result<(), Scope> {
        let stop = start + source.event_count();
        if stop <= floor || start >= limit {
            return Ok(());
        }
        if stop - start > 1 && source.is_sequential() {
            let begin = if start > floor { start } else { floor };
            let mut event = if stop < limit { stop } else { limit };
            let mut active = incoming[0] | incoming[1] | incoming[2] | incoming[3] != 0;
            while event > begin {
                event -= 1;
                if active || needed[event] != 0 {
                    match self.dependency_event(event, needed, incoming, path) {
                        Ok(()) => active = true,
                        Err(scope) => return Err(scope),
                    }
                }
            }
            return Ok(());
        }
        let active = incoming[0] | incoming[1] | incoming[2] | incoming[3] != 0;
        if !active {
            let begin = if start > floor { start } else { floor };
            let end = if stop < limit { stop } else { limit };
            // This is a backwards closure. Its retained upper bound already
            // ends at a needed action; find that action before walking an
            // unmarked prefix again at every right-nested source node.
            let mut event = end;
            while event > begin && needed[event - 1] == 0 {
                event -= 1;
            }
            if event == begin {
                return Ok(());
            }
        }
        match source {
            Source::Send(_) => self.dependency_event(start, needed, incoming, path),
            Source::Seq(left, right, _) => {
                let split = start + left.event_count();
                match self.dependency_prefix(right, (split, floor, limit), needed, incoming, path) {
                    Ok(()) => (),
                    Err(scope) => return Err(scope),
                }
                self.dependency_prefix(left, (start, floor, limit), needed, incoming, path)
            }
            Source::Par(left, right, _) => {
                let split = start + left.event_count();
                let mut left_after = *incoming;
                match self.dependency_prefix(
                    left,
                    (start, floor, limit),
                    needed,
                    &mut left_after,
                    path,
                ) {
                    Ok(()) => (),
                    Err(scope) => return Err(scope),
                }
                match self.dependency_prefix(right, (split, floor, limit), needed, incoming, path) {
                    Ok(()) => (),
                    Err(scope) => return Err(scope),
                }
                let mut word = 0;
                while word < ACTOR_WORDS {
                    incoming[word] |= left_after[word];
                    word += 1;
                }
                Ok(())
            }
            Source::Route { left, right, .. } => {
                let split = start + left.event_count();
                let scope = Scope {
                    start: start as u16,
                    split: split as u16,
                    stop: stop as u16,
                };
                match path.get(scope) {
                    Some(Arm::Left) => {
                        self.dependency_prefix(left, (start, floor, limit), needed, incoming, path)
                    }
                    Some(Arm::Right) => {
                        self.dependency_prefix(right, (split, floor, limit), needed, incoming, path)
                    }
                    None => {
                        let mut left_incoming = *incoming;
                        match self.dependency_prefix(
                            left,
                            (start, floor, limit),
                            needed,
                            &mut left_incoming,
                            &path.select(Selection {
                                scope,
                                arm: Arm::Left,
                            }),
                        ) {
                            Ok(()) => (),
                            Err(scope) => return Err(scope),
                        }
                        match self.dependency_prefix(
                            right,
                            (split, floor, limit),
                            needed,
                            incoming,
                            &path.select(Selection {
                                scope,
                                arm: Arm::Right,
                            }),
                        ) {
                            Ok(()) => (),
                            Err(scope) => return Err(scope),
                        }
                        // Each arm writes only its own source positions. An
                        // identical predecessor demand can be shared by the
                        // prefix; unequal demands still split the actual path.
                        let mut word = 0;
                        while word < ACTOR_WORDS {
                            if incoming[word] != left_incoming[word] {
                                return Err(scope);
                            }
                            word += 1;
                        }
                        Ok(())
                    }
                }
            }
            Source::Roll(body, _) => {
                self.dependency_prefix(body, (start, floor, limit), needed, incoming, path)
            }
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
        // Resolve the same path once into the existing split-indexed Context.
        // Repeated owner/actor queries then avoid walking a linked choice
        // prefix. Branch exploration mutates only this compiler workspace.
        let mut selections = [None; E];
        let mut split = 1;
        while split < self.image.event_len {
            let (scope, _) = self.route_parents[split];
            if scope.split != 0
                && let Some(arm) = path.get(scope)
            {
                selections[split] = Some(Selection { scope, arm });
            }
            split += 1;
        }
        self.selected_body(body, &mut selections, (None, None))
    }

    const fn selected_body(
        &self,
        body: &Body<'_>,
        selections: &mut [Option<Selection>; E],
        mut owners: (Option<u8>, Option<u8>),
    ) -> bool {
        // Once a partial path resolves a boundary owner to an actor, further
        // choices cannot change it. Do not rerun its whole-source query when
        // exploring an unrelated conditional departure or return.
        let scope = match self.run(body, &Context::from_index(selections), &mut owners) {
            Ok(result) => return result,
            Err(scope) => scope,
        };
        let slot = scope.split as usize;
        assert!(selections[slot].is_none());
        selections[slot] = Some(Selection {
            scope,
            arm: Arm::Left,
        });
        let mut valid = self.selected_body(body, selections, owners);
        if valid {
            selections[slot] = Some(Selection {
                scope,
                arm: Arm::Right,
            });
            valid = self.selected_body(body, selections, owners);
        }
        selections[slot] = None;
        valid
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
