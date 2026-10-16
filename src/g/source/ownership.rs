// Const ownership elaboration for a typed choreography source.
//
// The source is shared with `g` lowering, not a second application DSL.
// The output preserves logical endpoint identities and assigns each
// operation to the actor currently holding its endpoint.
//
// Scratch and output are caller-owned. Ownership follows the source prefix
// and its choices, without a mutable table of current holders. Queries here
// run during projection/reference checking, not on the runtime's hot path.

pub use super::tree::{EndpointKey, Message, Payload, Source};
#[path = "ownership/queries.rs"]
mod queries;
pub(crate) use queries::Context;
pub use queries::{Arm, Scope, Selection};
use queries::{At, Atom, Evaluation, Query};
#[path = "ownership/actors.rs"]
mod actors;
pub use actors::{ActorImage, compile_actor_image_into};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorKind {
    Capacity,
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

/// Validate the source and retain only compile-time ownership constraints.
pub const fn validate<const S: usize, const E: usize>(
    source: &Source<'_>,
    scratch: &mut Scratch<S, E>,
) -> Result<(), Error> {
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
    Ok(())
}
