//! Read-only proof observations of the production source validator.

use super::*;

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

/// Read-only inferred facts. No runtime ownership or cursor is stored here.
pub struct Plan<'a, const SYMBOLS: usize, const EVENTS: usize> {
    source: Option<&'a Source<'a>>,
    actors: ActorImage<EVENTS, EVENTS>,
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
            actors: ActorImage::new(),
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

    pub const fn actor_decision_count(&self) -> usize {
        self.actors.decision_count()
    }

    pub const fn has_branch_knowledge(&self) -> bool {
        match self.source {
            Some(source) => self.actors.branch_knowledge_conflict(source).is_none(),
            None => false,
        }
    }
    /// The production lane allocator's complete potential-actor set. This is
    /// a read-only construction observation, not current ownership or progress.
    pub const fn possible_actors(&self, event: usize) -> Option<[u8; 32]> {
        if event < self.event_len {
            Some(self.actors.possible_actors(event))
        } else {
            None
        }
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
        if !valid_selections(source, selections) {
            return None;
        }
        let Some(message) = self.messages[event] else {
            return None;
        };
        let mut indexed = [None; E];
        let mut index = 0;
        while index < selections.len() {
            let selection = selections[index];
            indexed[selection.scope.split as usize] = Some(selection);
            index += 1;
        }
        let context = Context::from_index(&indexed);
        if !active(source, 0, event, &context) {
            return None;
        }
        let Some((sender_actor, receiver_actor)) = self.actors.actors_at(event, selections) else {
            return None;
        };
        Some(Binding {
            message,
            sender_actor,
            receiver_actor,
        })
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
pub const fn compile_into<'a, const S: usize, const E: usize>(
    source: &'a Source<'a>,
    scratch: &mut Scratch<S, E>,
    output: &mut Plan<'a, S, E>,
) -> Result<(), Error> {
    output.source = None;
    output.event_len = 0;
    output.issuance_len = 0;
    if let Err(error) = compile_actor_image_into(source, scratch, &mut output.actors) {
        return Err(error);
    }
    let mut index = 0;
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
        Source::Seq(left, right, _)
        | Source::Par(left, right, _)
        | Source::Route { left, right, .. } => {
            let split = left.event_count();
            if before < split && after < split {
                source_precedes(left, before, after)
            } else if before >= split && after >= split {
                source_precedes(right, before - split, after - split)
            } else {
                matches!(source, Source::Seq(..)) && before < split && after >= split
            }
        }
        Source::Roll(body, _) => source_precedes(body, before, after),
    }
}

/// Structural membership of an event in explicitly selected branches. Missing
/// decisions leave a structural projection view; they never select an arm.
const fn active(source: &Source<'_>, start: usize, event: usize, context: &Context<'_>) -> bool {
    match source {
        Source::Send(_) => start == event,
        Source::Seq(left, right, _)
        | Source::Par(left, right, _)
        | Source::Route { left, right, .. } => {
            let split = start + left.event_count();
            if let Source::Route { .. } = source {
                let scope = Scope {
                    start: start as u16,
                    split: split as u16,
                    stop: (split + right.event_count()) as u16,
                };
                if let Some(arm) = context.get(scope) {
                    match arm {
                        Arm::Left if event >= split => return false,
                        Arm::Right if event < split => return false,
                        _ => {}
                    }
                }
            }
            if event < split {
                active(left, start, event, context)
            } else {
                active(right, split, event, context)
            }
        }
        Source::Roll(body, _) => active(body, start, event, context),
    }
}

const fn contains_scope(source: &Source<'_>, start: usize, scope: Scope) -> bool {
    match source {
        Source::Send(_) => false,
        Source::Seq(left, right, _)
        | Source::Par(left, right, _)
        | Source::Route { left, right, .. } => {
            let split = start + left.event_count();
            if let Source::Route { .. } = source
                && scope.same(Scope {
                    start: start as u16,
                    split: split as u16,
                    stop: (split + right.event_count()) as u16,
                })
            {
                return true;
            }
            contains_scope(left, start, scope) || contains_scope(right, split, scope)
        }
        Source::Roll(body, _) => contains_scope(body, start, scope),
    }
}

const fn valid_selections(source: &Source<'_>, selections: &[Selection]) -> bool {
    let mut index = 0;
    while index < selections.len() {
        if !contains_scope(source, 0, selections[index].scope) {
            return false;
        }
        let mut earlier = 0;
        while earlier < index {
            if selections[earlier].scope.same(selections[index].scope) {
                return false;
            }
            earlier += 1;
        }
        index += 1;
    }
    true
}
