#![no_std]

//! Executable reference model for ownership-aware endpoint projection.
//!
//! This proof package is not the Hibana runtime or an application DSL. Its
//! source constructors mirror `send`, `seq`, `par`, `route`/resolver and `roll`.
//! It checks an ownership elaboration before any owned-message API is added to
//! the core. The output preserves logical wire identities and assigns each
//! operation to the actor currently holding its endpoint.
//!
//! Scratch and output are caller-owned. Ownership follows the source prefix
//! and its choices, without a mutable table of current holders. Queries here
//! run during projection/reference checking, not on the runtime's hot path.

mod ownership;
pub use ownership::{Arm, Scope, Selection};
use ownership::{At, Atom, Context, Evaluation, Query};

/// A logical endpoint in a statically composed protocol family.
///
/// Component identity separates sessions; role identity names the same actor
/// across those components unless this endpoint is explicitly delegated.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EndpointKey {
    pub component: u16,
    pub role: u8,
}

impl EndpointKey {
    const fn same(self, other: Self) -> bool {
        self.component == other.component && self.role == other.role
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Payload {
    Wire { schema: u32 },
    Endpoint(EndpointKey),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Message {
    pub from: EndpointKey,
    pub to: EndpointKey,
    pub label: u8,
    pub payload: Payload,
}

#[derive(Clone, Copy, Debug)]
pub enum Source<'a> {
    Send(Message),
    Seq(&'a Self, &'a Self),
    Par(&'a Self, &'a Self),
    Route {
        left: &'a Self,
        right: &'a Self,
        resolver: Option<u16>,
    },
    Roll(&'a Self),
}

impl Source<'_> {
    pub const fn event_count(&self) -> usize {
        match self {
            Self::Send(_) => 1,
            Self::Seq(left, right) | Self::Par(left, right) | Self::Route { left, right, .. } => {
                left.event_count() + right.event_count()
            }
            Self::Roll(body) => body.event_count(),
        }
    }

    const fn uses(&self, key: EndpointKey) -> bool {
        match self {
            Self::Send(message) => {
                message.from.same(key)
                    || message.to.same(key)
                    || matches!(message.payload, Payload::Endpoint(payload) if payload.same(key))
            }
            Self::Seq(left, right) | Self::Par(left, right) | Self::Route { left, right, .. } => {
                left.uses(key) || right.uses(key)
            }
            Self::Roll(body) => body.uses(key),
        }
    }

    const fn transfers(&self, key: EndpointKey) -> bool {
        match self {
            Self::Send(message) => {
                matches!(message.payload, Payload::Endpoint(payload) if payload.same(key))
            }
            Self::Seq(left, right) | Self::Par(left, right) | Self::Route { left, right, .. } => {
                left.transfers(key) || right.transfers(key)
            }
            Self::Roll(body) => body.transfers(key),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorKind {
    Capacity,
    InvalidSource,
    NonUnitSelfMessage,
    EndpointIsItsOwnCarrier,
    ConflictingInitialOwners,
    UnrootedInitialOwner,
    ParallelOwnershipConflict,
    RollChangesOwner,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Error {
    pub kind: ErrorKind,
    pub endpoint: Option<EndpointKey>,
}

const fn error(kind: ErrorKind, endpoint: Option<EndpointKey>) -> Error {
    Error { kind, endpoint }
}

#[derive(Clone, Copy)]
enum EndpointKind {
    Control,
    Delegated,
}

#[derive(Clone, Copy)]
struct Symbol {
    key: EndpointKey,
    kind: EndpointKind,
}

#[derive(Clone, Copy, Debug)]
enum Owner {
    Actor(u8),
    Initial(u16),
}

#[derive(Clone, Copy)]
struct InitialOwner {
    parent: u16,
    rank: u8,
    actor: Option<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Binding {
    pub message: Message,
    pub sender_actor: u8,
    pub receiver_actor: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Issuance {
    pub endpoint: EndpointKey,
    pub actor: u8,
}

/// Compiler scratch, not a resident protocol or permission state.
pub struct Scratch<const SYMBOLS: usize, const EVENTS: usize> {
    symbols: [Option<Symbol>; SYMBOLS],
    symbol_len: usize,
    initial: [InitialOwner; SYMBOLS],
    events: [Option<Message>; EVENTS],
    event_len: usize,
}

impl<const S: usize, const E: usize> Default for Scratch<S, E> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const S: usize, const E: usize> Scratch<S, E> {
    pub const fn new() -> Self {
        Self {
            symbols: [None; S],
            symbol_len: 0,
            initial: [InitialOwner {
                parent: 0,
                rank: 0,
                actor: None,
            }; S],
            events: [None; E],
            event_len: 0,
        }
    }

    const fn index(&self, key: EndpointKey) -> Option<usize> {
        let mut index = 0;
        while index < self.symbol_len {
            if let Some(symbol) = self.symbols[index]
                && symbol.key.same(key)
            {
                return Some(index);
            }
            index += 1;
        }
        None
    }

    const fn collect_key(&mut self, key: EndpointKey, kind: EndpointKind) -> Result<(), Error> {
        if let Some(index) = self.index(key) {
            if let EndpointKind::Delegated = kind {
                self.symbols[index] = Some(Symbol { key, kind });
            }
            return Ok(());
        }
        if self.symbol_len == S || self.symbol_len >= u16::MAX as usize {
            return Err(error(ErrorKind::Capacity, Some(key)));
        }
        self.symbols[self.symbol_len] = Some(Symbol { key, kind });
        self.symbol_len += 1;
        Ok(())
    }

    const fn collect(&mut self, source: &Source<'_>) -> Result<(), Error> {
        match source {
            Source::Send(message) => {
                if let Err(error) = self.collect_key(message.from, EndpointKind::Control) {
                    return Err(error);
                }
                if let Err(error) = self.collect_key(message.to, EndpointKind::Control) {
                    return Err(error);
                }
                if let Payload::Endpoint(key) = message.payload
                    && let Err(error) = self.collect_key(key, EndpointKind::Delegated)
                {
                    return Err(error);
                }
                Ok(())
            }
            Source::Seq(left, right)
            | Source::Par(left, right)
            | Source::Route { left, right, .. } => {
                if let Err(error) = self.collect(left) {
                    return Err(error);
                }
                self.collect(right)
            }
            Source::Roll(body) => self.collect(body),
        }
    }

    const fn initialize_owners(&mut self) {
        let mut index = 0;
        while index < self.symbol_len {
            self.initial[index] = InitialOwner {
                parent: index as u16,
                rank: 0,
                actor: None,
            };
            index += 1;
        }
    }

    const fn root(&self, index: u16) -> usize {
        let mut root = index as usize;
        while self.initial[root].parent as usize != root {
            root = self.initial[root].parent as usize;
        }
        root
    }

    const fn resolve(&self, owner: Owner) -> Option<u8> {
        match owner {
            Owner::Actor(actor) => Some(actor),
            Owner::Initial(index) => self.initial[self.root(index)].actor,
        }
    }

    const fn unify(&mut self, left: Owner, right: Owner) -> bool {
        match (left, right) {
            (Owner::Actor(left), Owner::Actor(right)) => left == right,
            (Owner::Initial(index), Owner::Actor(actor))
            | (Owner::Actor(actor), Owner::Initial(index)) => {
                let root = self.root(index);
                match self.initial[root].actor {
                    Some(known) => known == actor,
                    None => {
                        self.initial[root].actor = Some(actor);
                        true
                    }
                }
            }
            (Owner::Initial(left), Owner::Initial(right)) => {
                let mut left = self.root(left);
                let mut right = self.root(right);
                if left == right {
                    return true;
                }
                if let (Some(left), Some(right)) =
                    (self.initial[left].actor, self.initial[right].actor)
                    && left != right
                {
                    return false;
                }
                if self.initial[left].rank < self.initial[right].rank {
                    core::mem::swap(&mut left, &mut right);
                }
                self.initial[right].parent = left as u16;
                if self.initial[left].actor.is_none() {
                    self.initial[left].actor = self.initial[right].actor;
                }
                if self.initial[left].rank == self.initial[right].rank {
                    self.initial[left].rank += 1;
                }
                true
            }
        }
    }

    const fn atom(&self, atom: Atom) -> Owner {
        match atom {
            Atom::Actor(actor) => Owner::Actor(actor),
            Atom::Initial(key) => {
                let Some(index) = self.index(key) else {
                    panic!("collected initial owner")
                };
                Owner::Initial(index as u16)
            }
        }
    }

    const fn equate(
        &mut self,
        source: &Source<'_>,
        left: At,
        right: At,
        context: &Context<'_>,
    ) -> bool {
        let query = Query::new(source);
        let left_owner = query.owner(left, context);
        let right_owner = query.owner(right, context);
        let scope = match (left_owner, right_owner) {
            (Evaluation::Atom(left), Evaluation::Atom(right)) => {
                return self.unify(self.atom(left), self.atom(right));
            }
            (Evaluation::Choice(scope), _) | (_, Evaluation::Choice(scope)) => scope,
        };
        let left_path = context.select(Selection {
            scope,
            arm: Arm::Left,
        });
        if !self.equate(source, left, right, &left_path) {
            return false;
        }
        let right_path = context.select(Selection {
            scope,
            arm: Arm::Right,
        });
        self.equate(source, left, right, &right_path)
    }

    const fn emit(
        &mut self,
        source: &Source<'_>,
        message: Message,
        context: &Context<'_>,
    ) -> Result<(), Error> {
        if self.event_len == E {
            return Err(error(ErrorKind::Capacity, None));
        }
        if message.from.same(message.to) && !matches!(message.payload, Payload::Wire { schema: 0 })
        {
            return Err(error(ErrorKind::NonUnitSelfMessage, Some(message.from)));
        }
        if let Payload::Endpoint(key) = message.payload {
            if key.same(message.from) || key.same(message.to) {
                return Err(error(ErrorKind::EndpointIsItsOwnCarrier, Some(key)));
            }
            let cut = self.event_len;
            if !self.equate(
                source,
                At { key, cut },
                At {
                    key: message.from,
                    cut,
                },
                context,
            ) {
                return Err(error(ErrorKind::ConflictingInitialOwners, Some(key)));
            }
        }
        self.events[self.event_len] = Some(message);
        self.event_len += 1;
        Ok(())
    }

    const fn visit(
        &mut self,
        root: &Source<'_>,
        source: &Source<'_>,
        context: &Context<'_>,
    ) -> Result<(), Error> {
        match source {
            Source::Send(message) => self.emit(root, *message, context),
            Source::Seq(left, right) => {
                if let Err(error) = self.visit(root, left, context) {
                    return Err(error);
                }
                self.visit(root, right, context)
            }
            Source::Par(left, right) => {
                let mut index = 0;
                while index < self.symbol_len {
                    let Some(symbol) = self.symbols[index] else {
                        panic!("collected symbol")
                    };
                    if (left.transfers(symbol.key) && right.uses(symbol.key))
                        || (right.transfers(symbol.key) && left.uses(symbol.key))
                    {
                        return Err(error(
                            ErrorKind::ParallelOwnershipConflict,
                            Some(symbol.key),
                        ));
                    }
                    index += 1;
                }
                if let Err(error) = self.visit(root, left, context) {
                    return Err(error);
                }
                self.visit(root, right, context)
            }
            Source::Route { left, right, .. } => {
                let start = self.event_len;
                let split = start + left.event_count();
                let stop = split + right.event_count();
                let scope = Scope {
                    start: start as u16,
                    split: split as u16,
                    stop: stop as u16,
                };
                let left_path = context.select(Selection {
                    scope,
                    arm: Arm::Left,
                });
                if let Err(error) = self.visit(root, left, &left_path) {
                    return Err(error);
                }
                let right_path = context.select(Selection {
                    scope,
                    arm: Arm::Right,
                });
                self.visit(root, right, &right_path)
            }
            Source::Roll(body) => {
                let start = self.event_len;
                if let Err(error) = self.visit(root, body, context) {
                    return Err(error);
                }
                let stop = self.event_len;
                let mut index = 0;
                while index < self.symbol_len {
                    let Some(symbol) = self.symbols[index] else {
                        panic!("collected symbol")
                    };
                    if body.transfers(symbol.key)
                        && !self.equate(
                            root,
                            At {
                                key: symbol.key,
                                cut: start,
                            },
                            At {
                                key: symbol.key,
                                cut: stop,
                            },
                            context,
                        )
                    {
                        return Err(error(ErrorKind::RollChangesOwner, Some(symbol.key)));
                    }
                    index += 1;
                }
                Ok(())
            }
        }
    }
}

/// Read-only inferred facts. No runtime ownership or cursor is stored here.
pub struct Plan<'a, const SYMBOLS: usize, const EVENTS: usize> {
    source: Option<&'a Source<'a>>,
    messages: [Option<Message>; EVENTS],
    event_len: usize,
    issuance: [Option<Issuance>; SYMBOLS],
    issuance_len: usize,
}

impl<const S: usize, const E: usize> Default for Plan<'_, S, E> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const S: usize, const E: usize> Plan<'_, S, E> {
    pub const fn new() -> Self {
        Self {
            source: None,
            messages: [None; E],
            event_len: 0,
            issuance: [None; S],
            issuance_len: 0,
        }
    }

    pub const fn len(&self) -> usize {
        self.event_len
    }
    pub const fn is_empty(&self) -> bool {
        self.event_len == 0
    }
    pub const fn binding(&self, event: usize) -> Option<Binding> {
        self.binding_in(event, &[])
    }

    /// Resolve a structural event using the source's route selections. A
    /// missing decision never picks a holder: the result is present only when
    /// all possible remaining decisions give the same actor.
    pub const fn binding_in(&self, event: usize, selections: &[Selection]) -> Option<Binding> {
        if event >= self.event_len {
            return None;
        }
        let Some(source) = self.source else {
            return None;
        };
        if !ownership::valid_selections(source, selections) {
            return None;
        }
        let Some(message) = self.messages[event] else {
            return None;
        };
        let context = Context::new(selections);
        if !ownership::active(source, 0, event, &context) {
            return None;
        }
        let Some(sender_actor) = self.actor(
            At {
                key: message.from,
                cut: event,
            },
            &context,
        ) else {
            return None;
        };
        let Some(receiver_actor) = self.actor(
            At {
                key: message.to,
                cut: event,
            },
            &context,
        ) else {
            return None;
        };
        Some(Binding {
            message,
            sender_actor,
            receiver_actor,
        })
    }

    const fn actor(&self, at: At, context: &Context<'_>) -> Option<u8> {
        let Some(source) = self.source else {
            return None;
        };
        let scope = match Query::new(source).owner(at, context) {
            Evaluation::Atom(Atom::Actor(actor)) => return Some(actor),
            Evaluation::Atom(Atom::Initial(key)) => {
                let mut index = 0;
                while index < self.issuance_len {
                    if let Some(issuance) = self.issuance[index]
                        && issuance.endpoint.same(key)
                    {
                        return Some(issuance.actor);
                    }
                    index += 1;
                }
                panic!("published initial owner")
            }
            Evaluation::Choice(scope) => scope,
        };
        let left_context = context.select(Selection {
            scope,
            arm: Arm::Left,
        });
        let Some(left) = self.actor(at, &left_context) else {
            return None;
        };
        let right_context = context.select(Selection {
            scope,
            arm: Arm::Right,
        });
        let Some(right) = self.actor(at, &right_context) else {
            return None;
        };
        if left == right { Some(left) } else { None }
    }
    pub const fn issuance(&self, index: usize) -> Option<Issuance> {
        if index < self.issuance_len {
            self.issuance[index]
        } else {
            None
        }
    }
}

/// Elaborate into supplied output, publishing no accepted partial plan.
///
/// Capacity comes from source event/symbol counts. Ownership after a route
/// follows its actual arm, including in the common continuation. A rolled
/// body must preserve ownership for every legal selection of its arms.
pub const fn compile_into<'a, const S: usize, const E: usize>(
    source: &'a Source<'a>,
    scratch: &mut Scratch<S, E>,
    output: &mut Plan<'a, S, E>,
) -> Result<(), Error> {
    output.source = None;
    output.event_len = 0;
    output.issuance_len = 0;
    scratch.symbol_len = 0;
    scratch.event_len = 0;
    if S > u16::MAX as usize || E > u16::MAX as usize || source.event_count() > E {
        return Err(error(ErrorKind::Capacity, None));
    }
    if let Err(error) = scratch.collect(source) {
        return Err(error);
    }
    scratch.initialize_owners();
    if let Err(error) = scratch.visit(source, source, &Context::new(&[])) {
        return Err(error);
    }
    let mut index = 0;
    while index < scratch.symbol_len {
        let Some(symbol) = scratch.symbols[index] else {
            panic!("collected symbol")
        };
        if let EndpointKind::Delegated = symbol.kind
            && scratch.resolve(Owner::Initial(index as u16)).is_none()
        {
            return Err(error(ErrorKind::UnrootedInitialOwner, Some(symbol.key)));
        }
        index += 1;
    }
    // Every symbolic actor is now rooted. Nothing below this point can reject.
    index = 0;
    while index < scratch.event_len {
        let Some(event) = scratch.events[index] else {
            panic!("emitted event")
        };
        output.messages[index] = Some(event);
        index += 1;
    }
    index = 0;
    while index < scratch.symbol_len {
        let Some(symbol) = scratch.symbols[index] else {
            panic!("collected symbol")
        };
        if let EndpointKind::Delegated = symbol.kind {
            let Some(actor) = scratch.resolve(Owner::Initial(index as u16)) else {
                panic!("rooted initial owner")
            };
            output.issuance[output.issuance_len] = Some(Issuance {
                endpoint: symbol.key,
                actor,
            });
            output.issuance_len += 1;
        }
        index += 1;
    }
    output.event_len = scratch.event_len;
    output.source = Some(source);
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Send,
    Receive,
    Local,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ActionId {
    pub event: usize,
    pub side: Side,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Action {
    pub actor: u8,
    pub endpoint: EndpointKey,
    pub message: Message,
    pub side: Side,
}

impl<const S: usize, const E: usize> Plan<'_, S, E> {
    pub const fn action(&self, id: ActionId) -> Option<Action> {
        self.action_in(id, &[])
    }

    pub const fn action_in(&self, id: ActionId, selections: &[Selection]) -> Option<Action> {
        let Some(binding) = self.binding_in(id.event, selections) else {
            return None;
        };
        let local = binding.message.from.same(binding.message.to);
        let (actor, endpoint) = match id.side {
            Side::Send if !local => (binding.sender_actor, binding.message.from),
            Side::Receive if !local => (binding.receiver_actor, binding.message.to),
            Side::Local if local => (binding.sender_actor, binding.message.from),
            _ => return None,
        };
        Some(Action {
            actor,
            endpoint,
            message: binding.message,
            side: id.side,
        })
    }

    /// Actor-local order in one structural visit. Parallel siblings have no
    /// order; sequential fragments include owned endpoint operations as well
    /// as the actor's original control endpoint. Roll freshness is a separate
    /// obligation, not inferred from these visit-local event ordinals.
    pub const fn precedes(&self, before: ActionId, after: ActionId) -> bool {
        self.precedes_in(before, after, &[])
    }

    pub const fn precedes_in(
        &self,
        before: ActionId,
        after: ActionId,
        selections: &[Selection],
    ) -> bool {
        let (Some(left), Some(right)) = (
            self.action_in(before, selections),
            self.action_in(after, selections),
        ) else {
            return false;
        };
        if left.actor != right.actor {
            return false;
        }
        if before.event == after.event {
            return matches!(before.side, Side::Send) && matches!(after.side, Side::Receive);
        }
        let Some(source) = self.source else {
            return false;
        };
        source_precedes(source, before.event, after.event)
    }
}

const fn source_precedes(source: &Source<'_>, before: usize, after: usize) -> bool {
    match source {
        Source::Send(_) => false,
        Source::Seq(left, right) | Source::Par(left, right) | Source::Route { left, right, .. } => {
            let split = left.event_count();
            if before < split && after < split {
                source_precedes(left, before, after)
            } else if before >= split && after >= split {
                source_precedes(right, before - split, after - split)
            } else {
                matches!(source, Source::Seq(_, _)) && before < split && after >= split
            }
        }
        Source::Roll(body) => source_precedes(body, before, after),
    }
}

#[cfg(test)]
mod tests;

#[cfg(kani)]
mod kani;
